mod config;
mod live;

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use latex_core::ast::Align;
use latex_core::Severity;
use latex_markdown::{transform_with_stats, BlockPolicy, InlinePolicy, Options as MdOptions};
use latex_term::{render, Fallback, Mode, Options as RenderOptions, RenderOutput, Theme};

#[derive(Debug, Parser)]
#[command(
    name = "agent-latex",
    version,
    about = "Render LaTeX math as Unicode/ANSI 2D art for terminal conversations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render a single LaTeX expression.
    Render(RenderArgs),
    /// Replace math spans in Markdown with rendered art.
    Md(MdArgs),
    /// Read Markdown from stdin and write the transformed text to stdout.
    Filter(FilterArgs),
    /// Stream stdin and repaint open math formulas as they grow.
    Live(LiveArgs),
    /// Run an existing TUI in a PTY and render inline math in place (hotkey toggles).
    Wrap(WrapArgs),
    /// Manage persistent settings.
    Config(ConfigArgs),
    /// Print terminal and build diagnostics.
    Doctor,
    /// Show a local formula gallery. No agent or API key required.
    Demo,
}

#[derive(Debug, Parser)]
struct RenderArgs {
    #[arg(allow_hyphen_values = true)]
    latex: Option<String>,

    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Use the compact single-line form.
    #[arg(long)]
    inline: bool,

    #[arg(long)]
    width: Option<usize>,

    #[arg(long, default_value = "auto")]
    theme: String,

    #[arg(long)]
    no_color: bool,

    #[arg(long, default_value_t = 0)]
    indent: usize,

    #[arg(long)]
    center: bool,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TransformArgs {
    #[arg(long)]
    width: Option<usize>,

    #[arg(long, default_value = "auto")]
    theme: String,

    #[arg(long)]
    no_color: bool,

    /// Leave inline `$...$` and `\(...\)` untouched.
    #[arg(long)]
    preserve_inline: bool,

    /// Leave display math untouched.
    #[arg(long)]
    preserve_block: bool,

    /// Append the original TeX source under block art.
    #[arg(long)]
    keep_source: bool,

    /// Emit raw art instead of wrapping block math in a Markdown text fence.
    #[arg(long)]
    plain: bool,

    #[arg(long)]
    center: bool,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct MdArgs {
    file: Option<PathBuf>,

    #[command(flatten)]
    transform: TransformArgs,
}

#[derive(Debug, Parser)]
struct LiveArgs {
    #[arg(long)]
    width: Option<usize>,

    #[arg(long, default_value = "auto")]
    theme: String,

    #[arg(long)]
    no_color: bool,
}

#[derive(Debug, Parser)]
struct WrapArgs {
    /// Disable inline math replacement; useful for verifying the PTY layer.
    #[arg(long)]
    no_inline: bool,

    /// Disable the hotkey that toggles rendered/original output.
    #[arg(long)]
    no_hotkeys: bool,

    /// Key that toggles rendered/original output: f1..f12, ctrl+<letter>,
    /// alt+<letter>, or a raw escape sequence. Overrides the saved setting.
    #[arg(long, value_name = "KEY")]
    toggle_key: Option<String>,

    #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print the configuration file path.
    Path,
    /// Print a saved setting.
    Get { key: String },
    /// Save a setting.
    Set { key: String, value: String },
}

#[derive(Debug, Parser)]
struct FilterArgs {
    #[command(flatten)]
    transform: TransformArgs,
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code.min(255) as i32),
        Err(error) => {
            if is_broken_pipe(&error) {
                std::process::exit(0);
            }
            eprintln!("agent-latex: {error:#}");
            std::process::exit(1);
        }
    }
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
            .unwrap_or(false)
    })
}

fn run() -> Result<u32> {
    let cli = Cli::parse();
    match cli.command {
        Command::Render(args) => run_render(args),
        Command::Md(args) => run_md(args),
        Command::Filter(args) => run_filter(args),
        Command::Live(args) => run_live(args),
        Command::Wrap(args) => return run_wrap(args),
        Command::Config(args) => run_config(args.command),
        Command::Doctor => run_doctor(),
        Command::Demo => transform_and_write(
            include_str!("../../../examples/math.md"),
            &TransformArgs {
                width: None,
                theme: "auto".into(),
                no_color: false,
                preserve_inline: false,
                preserve_block: false,
                keep_source: false,
                plain: true,
                center: false,
                json: false,
            },
        ),
    }?;
    Ok(0)
}

fn run_render(args: RenderArgs) -> Result<()> {
    let source = read_input(args.latex.as_deref(), args.file.as_deref())?;
    let theme = build_theme(&args.theme, args.no_color)?;
    let width = effective_width(args.width);
    let options = RenderOptions {
        mode: if args.inline {
            Mode::Inline
        } else {
            Mode::Block
        },
        theme,
        max_width: width,
        align: if args.center {
            Align::Center
        } else {
            Align::Left
        },
        indent: args.indent,
        fallback: Fallback::Source,
    };
    let output = render(&source, &options);

    if args.json {
        write_json(&render_json(&source, &output))?;
    } else {
        write_stdout(&output.text)?;
    }
    Ok(())
}

fn run_md(args: MdArgs) -> Result<()> {
    let source = read_input(None, args.file.as_deref())?;
    transform_and_write(&source, &args.transform)
}

fn run_live(args: LiveArgs) -> Result<()> {
    let theme = build_theme(&args.theme, args.no_color)?;
    let width = if std::io::stdout().is_terminal() {
        effective_width(args.width).or(Some(80))
    } else {
        args.width
    };
    live::run(live::LiveOptions { width, theme })
}

fn run_wrap(args: WrapArgs) -> Result<u32> {
    if !std::io::stdout().is_terminal() {
        bail!("wrap must run with a terminal on stdout");
    }
    let math_policy = math_policy_for(&args.command);
    let toggle_key = if args.no_hotkeys || args.no_inline {
        None
    } else {
        let configured = config::load()?.toggle_key;
        let name = args
            .toggle_key
            .or(configured)
            .unwrap_or_else(|| "f12".to_string());
        Some(
            latex_pty::hotkey::parse_key(&name)
                .with_context(|| format!("unknown toggle key `{name}`"))?,
        )
    };
    latex_pty::run(latex_pty::WrapOptions {
        command: args.command,
        replace_inline: !args.no_inline,
        toggle_key,
        math_policy,
    })
}

fn math_policy_for(command: &[String]) -> latex_pty::math::MathPolicy {
    let name = command
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if matches!(name, "opencode" | "claude") {
        latex_pty::math::MathPolicy::Agent
    } else {
        latex_pty::math::MathPolicy::Generic
    }
}

fn run_config(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Path => println!("{}", config::path().display()),
        ConfigCommand::Get { key } => {
            let config = config::load()?;
            match key.as_str() {
                "toggle-key" => println!("{}", config.toggle_key.as_deref().unwrap_or("f12")),
                _ => bail!("unknown setting `{key}`; try `toggle-key`"),
            }
        }
        ConfigCommand::Set { key, value } => {
            if key != "toggle-key" {
                bail!("unknown setting `{key}`; try `toggle-key`");
            }
            if latex_pty::hotkey::parse_key(&value).is_none() {
                bail!("unknown toggle key `{value}`");
            }
            let path = config::set_toggle_key(&value)?;
            println!("toggle-key = {value}");
            println!("config: {}", path.display());
        }
    }
    Ok(())
}

fn run_filter(args: FilterArgs) -> Result<()> {
    let source = read_input(None, None)?;
    transform_and_write(&source, &args.transform)
}

fn transform_and_write(source: &str, args: &TransformArgs) -> Result<()> {
    let theme = build_theme(&args.theme, args.no_color)?;
    let options = MdOptions {
        width: effective_width(args.width),
        theme,
        inline_policy: if args.preserve_inline {
            InlinePolicy::Preserve
        } else {
            InlinePolicy::Render
        },
        block_policy: if args.preserve_block {
            BlockPolicy::Preserve
        } else {
            BlockPolicy::Render
        },
        keep_source: args.keep_source,
        fence: !args.plain,
        align: if args.center {
            Align::Center
        } else {
            Align::Left
        },
    };
    let (text, stats) = transform_with_stats(source, &options);

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "text": text,
            "stats": {
                "total": stats.total,
                "rendered": stats.rendered,
                "fallback": stats.fallback,
            }
        });
        write_json(&value)?;
    } else {
        write_stdout(&text)?;
    }
    Ok(())
}

fn run_doctor() -> Result<()> {
    let terminal = terminal_size::terminal_size()
        .map(|(width, height)| format!("{}x{}", width.0, height.0))
        .unwrap_or_else(|| "unknown".to_string());
    let theme = build_theme("auto", false)?;
    let stdout_terminal = std::io::stdout().is_terminal();

    println!("agent-latex {}", env!("CARGO_PKG_VERSION"));
    println!("terminal: {terminal}");
    println!("stdout is a terminal: {stdout_terminal}");
    println!(
        "TERM={} COLORTERM={} NO_COLOR={}",
        env_or("TERM"),
        env_or("COLORTERM"),
        std::env::var_os("NO_COLOR").map_or("unset", |_| "set")
    );
    println!("theme colour enabled: {}", theme.color_enabled());

    let inline = render(
        r"x^2 + \frac{1}{2}",
        &RenderOptions {
            mode: Mode::Inline,
            theme: theme.clone(),
            max_width: None,
            align: Align::Left,
            indent: 0,
            fallback: Fallback::Source,
        },
    );
    let block = render(
        r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
        &RenderOptions {
            mode: Mode::Block,
            theme,
            max_width: None,
            align: Align::Left,
            indent: 4,
            fallback: Fallback::Source,
        },
    );
    println!("\ninline sample:\n{}", inline.text);
    println!("\nblock sample:\n{}", block.text);
    Ok(())
}

fn read_input(latex: Option<&str>, file: Option<&std::path::Path>) -> Result<String> {
    if let Some(latex) = latex {
        return Ok(latex.to_string());
    }
    if let Some(file) = file {
        return std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()));
    }
    let mut source = String::new();
    std::io::stdin()
        .read_to_string(&mut source)
        .context("failed to read stdin")?;
    Ok(source)
}

fn build_theme(name: &str, no_color: bool) -> Result<Theme> {
    let Some(mut theme) = Theme::from_name(name) else {
        bail!("unknown theme '{name}' (expected auto, mono, ansi, dark or light)");
    };
    // `auto` follows the output destination: a pipe or file should not contain
    // escape sequences by accident. Use `--theme ansi` to force colour.
    if name == "auto" && !std::io::stdout().is_terminal() {
        theme = theme.with_color(false);
    }
    if no_color {
        theme = theme.with_color(false);
    }
    Ok(theme)
}

fn effective_width(explicit: Option<usize>) -> Option<usize> {
    explicit.or_else(|| {
        if std::io::stdout().is_terminal() {
            terminal_size::terminal_size().map(|(width, _)| width.0 as usize)
        } else {
            None
        }
    })
}

fn write_stdout(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    if !text.ends_with('\n') && !text.is_empty() {
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}

fn write_json(value: &serde_json::Value) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

fn render_json(source: &str, output: &RenderOutput) -> serde_json::Value {
    let diagnostics: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| {
            serde_json::json!({
                "severity": match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                "message": diagnostic.message,
                "span": diagnostic.span.map(|(start, end)| serde_json::json!([start, end])),
            })
        })
        .collect();

    serde_json::json!({
        "ok": !output.fallback,
        "source": source,
        "text": output.text,
        "width": output.width,
        "height": output.height,
        "baseline": output.baseline,
        "fallback": output.fallback,
        "diagnostics": diagnostics,
    })
}

fn env_or(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unset".to_string())
}
