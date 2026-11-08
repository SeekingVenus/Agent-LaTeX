//! Spawn a child in a PTY and composite its screen.

use std::io::{IsTerminal, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, select, tick, unbounded};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use signal_hook::consts::SIGWINCH;
use signal_hook::iterator::Signals;

use crate::compositor::{Compositor, Frame};

pub struct WrapOptions {
    pub command: Vec<String>,
    pub replace_inline: bool,
    /// Optional byte sequence that toggles rendered/original output.
    pub toggle_key: Option<Vec<u8>>,
    /// Math detection policy.
    pub math_policy: crate::math::MathPolicy,
}

pub fn run(options: WrapOptions) -> Result<u32> {
    if options.command.is_empty() {
        bail!("wrap requires a command, e.g. `agent-latex wrap -- opencode`");
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("wrap needs a terminal on stdin and stdout; use `md` or `live` for pipes");
    }

    let (cols, rows) = terminal_size::terminal_size()
        .map(|(width, height)| (width.0, height.0))
        .unwrap_or((80, 24));
    let pty_size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(pty_size).context("failed to open PTY")?;

    let command = build_command(&options);
    let child = pair
        .slave
        .spawn_command(command)
        .context("failed to spawn child in PTY")?;
    let mut child = ChildGuard(Some(child));
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("failed to take PTY writer")?;
    let master = Arc::new(Mutex::new(pair.master));
    let writer = Arc::new(Mutex::new(writer));

    // Bounded so a flooding child blocks on the PTY instead of exhausting
    // memory in an unbounded queue.
    let (output_tx, output_rx) = bounded(64);
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if output_tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let (toggle_tx, toggle_rx) = unbounded();
    let (resize_tx, resize_rx) = unbounded();
    let mut signals = Signals::new([SIGWINCH]).context("failed to watch SIGWINCH")?;
    thread::spawn(move || {
        for _ in signals.forever() {
            if let Some((width, height)) = terminal_size::terminal_size() {
                let _ = resize_tx.send((height.0, width.0));
            }
        }
    });

    let input_writer = Arc::clone(&writer);
    let key_filter = options.toggle_key.map(KeyFilter::new);
    thread::spawn(move || {
        let stdin = std::io::stdin();
        forward_input(stdin.as_raw_fd(), input_writer, key_filter, toggle_tx);
    });

    crossterm::terminal::enable_raw_mode().context("failed to enable raw mode")?;
    let _guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )
    .context("failed to enter alternate screen")?;

    let mut compositor = Compositor::new(rows, cols, options.replace_inline);
    compositor.set_math_policy(options.math_policy);

    // Coalesce bursts of child output into at most one frame per ~16 ms.
    // Streaming agents and `cat` otherwise trigger a full-screen redraw for
    // every read.
    let ticker = tick(Duration::from_millis(16));
    let mut dirty = false;
    let mut last_frame: Option<Frame> = None;
    let mut mode_forwarder = ModeForwarder::new();

    loop {
        select! {
            recv(output_rx) -> message => {
                match message {
                    Ok(bytes) => {
                        let mut modes = Vec::new();
                        mode_forwarder.feed(&bytes, &mut modes);
                        if !modes.is_empty() {
                            stdout.write_all(&modes)?;
                            stdout.flush()?;
                        }
                        compositor.process(&bytes);
                        respond_to_queries(&bytes, &writer, compositor.cursor());
                        dirty = true;
                    }
                    Err(_) => break,
                }
            }
            recv(resize_rx) -> message => {
                if let Ok((new_rows, new_cols)) = message {
                    if new_rows == 0 || new_cols == 0 {
                        continue;
                    }
                    if let Ok(master) = master.lock() {
                        let _ = master.resize(PtySize {
                            rows: new_rows,
                            cols: new_cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    compositor.resize(new_rows, new_cols);
                    dirty = true;
                }
            }
            recv(toggle_rx) -> message => {
                if message.is_ok() {
                    compositor.toggle_rendering();
                    dirty = true;
                }
            }
            recv(ticker) -> _ => {
                if dirty {
                    let frame = compositor.render_frame();
                    let bytes = frame.diff(last_frame.as_ref());
                    if !bytes.is_empty() {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                    }
                    last_frame = Some(frame);
                    dirty = false;
                }
            }
        }
    }

    if dirty {
        let frame = compositor.render_frame();
        let bytes = frame.diff(last_frame.as_ref());
        if !bytes.is_empty() {
            stdout.write_all(&bytes)?;
            stdout.flush()?;
        }
    }

    drop(_guard);
    let status = child
        .0
        .as_mut()
        .unwrap()
        .wait()
        .context("failed to wait for child")?;
    child.0 = None;
    Ok(status.exit_code())
}

// A failed terminal setup or repaint must not leave the agent running unseen.
struct ChildGuard(Option<Box<dyn portable_pty::Child + Send + Sync>>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Read keyboard bytes and forward them to the child. A configured hotkey is
/// removed; a held prefix is flushed after a short timeout or at EOF.
fn forward_input(
    fd: RawFd,
    input_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    mut key_filter: Option<KeyFilter>,
    toggle_tx: crossbeam_channel::Sender<()>,
) {
    let mut buffer = [0u8; 4096];
    loop {
        let mut forward = Vec::new();

        // With a hotkey filter, wait briefly for the rest of a possibly split
        // key. A lone Escape is flushed on timeout instead of being held until
        // the next key arrives.
        if key_filter.is_some() {
            let timeout = if key_filter.as_ref().is_some_and(KeyFilter::has_pending) {
                50
            } else {
                -1
            };
            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pollfd, 1, timeout) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if ready == 0 {
                if let Some(filter) = &mut key_filter {
                    filter.flush(&mut forward);
                }
                if forward.is_empty() {
                    continue;
                }
                if !forward_bytes(&input_writer, &forward) {
                    break;
                }
                continue;
            }
        }

        // Read the raw fd, not `Stdin`: mixing the buffered `Stdin` reader with
        // `poll` can strand bytes in its private buffer.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if read == 0 {
            // EOF: emit a held partial key before closing the input side.
            if let Some(filter) = &mut key_filter {
                filter.flush(&mut forward);
            }
            let _ = forward_bytes(&input_writer, &forward);
            break;
        }
        let read = read as usize;
        let toggles = match &mut key_filter {
            Some(filter) => filter.filter(&buffer[..read], &mut forward),
            None => {
                forward.extend_from_slice(&buffer[..read]);
                0
            }
        };
        if !forward_bytes(&input_writer, &forward) {
            break;
        }
        for _ in 0..toggles {
            let _ = toggle_tx.send(());
        }
    }
}

fn forward_bytes(input_writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let Ok(mut writer) = input_writer.lock() else {
        return false;
    };
    if writer.write_all(bytes).is_err() {
        return false;
    }
    let _ = writer.flush();
    true
}

const FORWARDED_MODES: &[&[u8]] = &[
    b"\x1b[?2004h",
    b"\x1b[?2004l",
    b"\x1b[?1004h",
    b"\x1b[?1004l",
];

/// Forward bracketed-paste and focus-reporting mode changes from the child to
/// the real terminal. The sequences may be split across reads.
struct ModeForwarder {
    pending: Vec<u8>,
}

impl ModeForwarder {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        self.pending.extend_from_slice(input);
        let mut index = 0usize;
        while index < self.pending.len() {
            let rest = &self.pending[index..];
            if let Some(mode) = FORWARDED_MODES.iter().find(|mode| rest.starts_with(mode)) {
                out.extend_from_slice(mode);
                index += mode.len();
                continue;
            }
            if FORWARDED_MODES
                .iter()
                .any(|mode| mode.len() > rest.len() && mode.starts_with(rest))
            {
                break;
            }
            index += 1;
        }
        self.pending.drain(..index);
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = std::io::stdout();
        // The child may have enabled these and exited without disabling them.
        let _ = stdout.write_all(b"\x1b[?2004l\x1b[?1004l");
        let _ = stdout.flush();
        let _ = crossterm::execute!(
            stdout,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

fn build_command(options: &WrapOptions) -> CommandBuilder {
    let program = &options.command[0];
    let mut command = CommandBuilder::new(program);
    for arg in &options.command[1..] {
        command.arg(arg);
    }
    // portable-pty defaults the child's cwd to $HOME when none is set. That
    // breaks relative paths (`wrap -- less README.md`). Always inherit the
    // directory the wrapper was launched from.
    if let Ok(directory) = std::env::current_dir() {
        command.env("PWD", &directory);
        command.cwd(&directory);
    }
    command
}

fn respond_to_queries(
    bytes: &[u8],
    writer: &Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    cursor: (u16, u16),
) {
    let mut response = Vec::new();

    if contains(bytes, b"\x1b[6n") {
        response.extend_from_slice(format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1).as_bytes());
    }
    if contains(bytes, b"\x1b[c") || contains(bytes, b"\x1b[0c") {
        response.extend_from_slice(b"\x1b[?1;2c");
    }
    if contains(bytes, b"\x1b]10;?") {
        response.extend_from_slice(b"\x1b]10;rgb:ffff/ffff/ffff\x07");
    }
    if contains(bytes, b"\x1b]11;?") {
        response.extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x07");
    }

    if response.is_empty() {
        return;
    }
    if let Ok(mut writer) = writer.lock() {
        let _ = writer.write_all(&response);
        let _ = writer.flush();
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

struct KeyFilter {
    pattern: Vec<u8>,
    pending: Vec<u8>,
}

impl KeyFilter {
    fn new(pattern: Vec<u8>) -> Self {
        Self {
            pattern,
            pending: Vec::new(),
        }
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Emit a held partial sequence. Called after a short input timeout, so a
    /// lone Escape reaches the child quickly.
    fn flush(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.pending);
        self.pending.clear();
    }

    /// Returns the number of toggle-key occurrences found in this chunk.
    fn filter(&mut self, input: &[u8], out: &mut Vec<u8>) -> usize {
        if self.pattern.is_empty() {
            out.extend_from_slice(input);
            return 0;
        }

        self.pending.extend_from_slice(input);
        let mut toggles = 0usize;
        while let Some(position) = find_subslice(&self.pending, &self.pattern) {
            out.extend_from_slice(&self.pending[..position]);
            self.pending.drain(..position + self.pattern.len());
            toggles += 1;
        }

        // Hold only a suffix that could be the start of the toggle sequence.
        let max = self.pattern.len().saturating_sub(1).min(self.pending.len());
        let mut keep = 0usize;
        for length in (1..=max).rev() {
            if self.pending.ends_with(&self.pattern[..length]) {
                keep = length;
                break;
            }
        }
        let flush = self.pending.len() - keep;
        out.extend_from_slice(&self.pending[..flush]);
        self.pending.drain(..flush);
        toggles
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
