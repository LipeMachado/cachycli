//! Small text helpers shared by the tools and the interface.

/// The largest byte index `<= max` that falls on a character boundary.
///
/// Truncating with `&text[..n]` panics when `n` lands inside a multi-byte
/// character, which is easy to hit with accents, emoji or CJK text — exactly
/// what a tool output or a fetched page tends to contain.
pub fn floor_char_boundary(text: &str, max: usize) -> usize {
    if max >= text.len() {
        return text.len();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// `text` cut to at most `max` bytes, never inside a character.
pub fn cut(text: &str, max: usize) -> &str {
    &text[..floor_char_boundary(text, max)]
}

/// A short, printable form of an id or name (used for file names).
pub fn short(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_splits_a_character() {
        let text = "café ☕ 日本語";
        // Every prefix length must be usable as a slice.
        for limit in 0..=text.len() {
            let cut_text = cut(text, limit);
            assert!(text.starts_with(cut_text));
            assert!(cut_text.len() <= limit);
        }
        assert_eq!(cut(text, 3), "caf");
        assert_eq!(cut(text, 4), "caf"); // 'é' is two bytes: stop before it
        assert_eq!(cut(text, 5), "café");
        assert_eq!(cut(text, 1000), text);
        assert_eq!(cut(text, 0), "");
    }

    #[test]
    fn short_counts_characters_not_bytes() {
        assert_eq!(short("sessão-longa", 6), "sessão");
        assert_eq!(short("日本語のセッション", 3), "日本語");
        assert_eq!(short("", 4), "");
    }
}
