//! Markdown-ish helpers shared by the streaming, whole-document and screen
//! scanners. Keeping one copy means a delimiter fix reaches every surface.

/// A `$...$` span is only math when it has no surrounding whitespace and does
/// not cross a line.
pub fn valid_inline(source: &str) -> bool {
    if source.is_empty() || source.contains('\n') {
        return false;
    }
    if source.starts_with(char::is_whitespace) || source.ends_with(char::is_whitespace) {
        return false;
    }
    true
}

pub fn find_unescaped(input: &str, start: usize, pattern: &str) -> Option<usize> {
    let mut cursor = start;
    while let Some(offset) = input[cursor..].find(pattern) {
        let index = cursor + offset;
        if !is_escaped(input, index) {
            return Some(index);
        }
        cursor = index + pattern.len();
    }
    None
}

/// A fence opening line: marker character, marker run length, and info string.
pub fn parse_fence_line(line: &str) -> Option<(char, usize, String)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let marker_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if marker_len < 3 {
        return None;
    }
    let info = trimmed[marker_len..].trim().to_string();
    Some((marker, marker_len, info))
}

pub fn is_closing_fence_line(line: &str, marker: char, marker_len: usize) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= marker_len && trimmed[count..].trim().is_empty()
}

pub fn is_escaped(input: &str, index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if input.as_bytes()[cursor] == b'\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}
