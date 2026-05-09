//! ANSI escape sequence stripping.

/// Strip ANSI escape sequences from a string.
///
/// Handles both raw ESC byte (`\x1b[...m`) and JSON-encoded form
/// (`\\u001b[...m`).
pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(letter) = chars.next() {
        if letter == '\x1b' {
            consume_until_m(&mut chars);
        } else if letter == '\\' && chars.peek() == Some(&'u') {
            let lookahead: String = chars.clone().take(5).collect();
            if lookahead.starts_with("u001b") || lookahead.starts_with("u001B") {
                for _ in 0..5_u8 {
                    chars.next();
                }
                consume_until_m(&mut chars);
            } else {
                result.push(letter);
            }
        } else {
            result.push(letter);
        }
    }
    result
}

/// Drain `chars` up to and including the next `'m'`.
fn consume_until_m<I: Iterator<Item = char>>(chars: &mut I) {
    for next in chars {
        if next == 'm' {
            break;
        }
    }
}
