//! Named terminal keys for the rendered/original toggle.

/// Parse a key name into the byte sequence the terminal sends.
///
/// Supported forms:
///
/// - `f1` .. `f12`
/// - `ctrl+<letter>` (and a few punctuation keys: `ctrl+space`, `ctrl+[`, ...)
/// - `alt+<letter>`
/// - a raw escape sequence, for terminals with unusual encodings
pub fn parse_key(input: &str) -> Option<Vec<u8>> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.starts_with('\x1b') {
        return Some(input.as_bytes().to_vec());
    }

    let lower = input.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("ctrl+") {
        return ctrl_key(rest).map(|byte| vec![byte]);
    }
    if let Some(rest) = lower.strip_prefix("alt+") {
        if rest.len() == 1 && rest.is_ascii() {
            return Some(vec![0x1b, rest.as_bytes()[0]]);
        }
        return None;
    }
    function_key(&lower)
}

fn ctrl_key(name: &str) -> Option<u8> {
    match name {
        "space" | "@" => Some(0x00),
        "[" => Some(0x1b),
        "\\" => Some(0x1c),
        "]" => Some(0x1d),
        "^" => Some(0x1e),
        "_" => Some(0x1f),
        "?" => Some(0x7f),
        _ => {
            let mut chars = name.chars();
            let ch = chars.next()?.to_ascii_lowercase();
            if chars.next().is_some() || !ch.is_ascii_lowercase() {
                return None;
            }
            Some((ch as u8) & 0x1f)
        }
    }
}

fn function_key(name: &str) -> Option<Vec<u8>> {
    let number: u8 = name.strip_prefix('f')?.parse().ok()?;
    let sequence = match number {
        1 => "\x1bOP",
        2 => "\x1bOQ",
        3 => "\x1bOR",
        4 => "\x1bOS",
        5 => "\x1b[15~",
        6 => "\x1b[17~",
        7 => "\x1b[18~",
        8 => "\x1b[19~",
        9 => "\x1b[20~",
        10 => "\x1b[21~",
        11 => "\x1b[23~",
        12 => "\x1b[24~",
        _ => return None,
    };
    Some(sequence.as_bytes().to_vec())
}
