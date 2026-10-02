//! Bounded presentation-only case conversion. Canonical text is never rewritten.
use std::borrow::Cow;

pub const MAX_TEXT_BYTES: usize = 65_536;

/// Verified plain case-conversion subset of Leafer's textCase contract.
/// Title and small-caps are intentionally unsupported: neither is uppercase.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextCase {
    #[default]
    None,
    Upper,
    Lower,
}
impl TextCase {
    /// Transform each Unicode scalar independently, as the pinned upstream
    /// TextRows/TextCase pipeline does. In particular Lower does not apply
    /// whole-string contextual final-sigma or locale-sensitive casing.
    pub fn presentation(self, content: &str) -> Result<Cow<'_, str>, String> {
        if self == Self::None {
            return Ok(Cow::Borrowed(content));
        }
        if content.len() > MAX_TEXT_BYTES {
            return Err("case conversion source exceeds 64KiB UTF-8".into());
        }
        let bytes: usize = match self {
            Self::Upper => content
                .chars()
                .flat_map(char::to_uppercase)
                .map(char::len_utf8)
                .sum(),
            Self::Lower => content
                .chars()
                .flat_map(char::to_lowercase)
                .map(char::len_utf8)
                .sum(),
            Self::None => unreachable!(),
        };
        if bytes > MAX_TEXT_BYTES {
            return Err("case conversion output exceeds 64KiB UTF-8".into());
        }
        let mut output = String::new();
        output
            .try_reserve_exact(bytes)
            .map_err(|_| "case conversion allocation")?;
        match self {
            Self::Upper => output.extend(content.chars().flat_map(char::to_uppercase)),
            Self::Lower => output.extend(content.chars().flat_map(char::to_lowercase)),
            Self::None => unreachable!(),
        }
        Ok(Cow::Owned(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_case_is_scalar_based_and_preserves_source_breaks_and_graphemes() {
        let source = "Straße ﬃ İ ΟΣ\ne\u{301} 中文 👩‍👩‍👧‍👦\n";
        assert_eq!(
            TextCase::Upper.presentation(source).unwrap(),
            "STRASSE FFI İ ΟΣ\nE\u{301} 中文 👩‍👩‍👧‍👦\n"
        );
        assert_eq!(
            TextCase::Lower.presentation(source).unwrap(),
            "straße ﬃ i\u{307} οσ\ne\u{301} 中文 👩‍👩‍👧‍👦\n"
        );
        assert!(matches!(
            TextCase::None.presentation(source),
            Ok(Cow::Borrowed(_))
        ));
    }
    #[test]
    fn case_budget_checks_input_and_expansion_before_allocating() {
        assert!(
            TextCase::Upper
                .presentation(&"x".repeat(MAX_TEXT_BYTES))
                .is_ok()
        );
        assert!(
            TextCase::Upper
                .presentation(&"x".repeat(MAX_TEXT_BYTES + 1))
                .is_err()
        );
        // U+0149 is two UTF-8 bytes, uppercase U+02BC + N is three.
        let source = "ŉ".repeat(MAX_TEXT_BYTES / 2);
        assert!(source.len() <= MAX_TEXT_BYTES);
        assert!(TextCase::Upper.presentation(&source).is_err());
        assert!(
            TextCase::Lower
                .presentation(&"İ".repeat(MAX_TEXT_BYTES / 2))
                .is_err()
        );
        assert!(
            TextCase::None
                .presentation(&"x".repeat(MAX_TEXT_BYTES + 1))
                .is_ok()
        );
    }
}
