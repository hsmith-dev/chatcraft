//! Name rules for channels and handles (lowercase, no spaces, bounded length).

use crate::limits::{MAX_CHANNEL_NAME, MAX_HANDLE};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("name is empty")]
    Empty,
    #[error("name is longer than {0} characters")]
    TooLong(usize),
    #[error("names may only contain lowercase letters, numbers, hyphens and underscores")]
    BadChar,
}

fn normalize(raw: &str, max: usize, strip: char) -> Result<String, NameError> {
    let s = raw.trim().trim_start_matches(strip).trim();
    if s.is_empty() {
        return Err(NameError::Empty);
    }
    // Spaces become hyphens, as people type "launch plan" for #launch-plan.
    let s: String = s.chars().map(|c| if c.is_whitespace() { '-' } else { c }).flat_map(char::to_lowercase).collect();
    if s.chars().count() > max {
        return Err(NameError::TooLong(max));
    }
    if !s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(NameError::BadChar);
    }
    Ok(s)
}

/// `"#Launch Plan"` → `"launch-plan"`.
pub fn normalize_channel_name(raw: &str) -> Result<String, NameError> {
    normalize(raw, MAX_CHANNEL_NAME, '#')
}

/// `"@Ada"` → `"ada"`.
pub fn normalize_handle(raw: &str) -> Result<String, NameError> {
    normalize(raw, MAX_HANDLE, '@')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_normalize() {
        assert_eq!(normalize_channel_name("#Launch Plan").unwrap(), "launch-plan");
        assert_eq!(normalize_channel_name("  dev_ops ").unwrap(), "dev_ops");
        assert_eq!(normalize_channel_name("#"), Err(NameError::Empty));
        assert_eq!(normalize_channel_name("a/b"), Err(NameError::BadChar));
        assert_eq!(normalize_channel_name(&"x".repeat(81)), Err(NameError::TooLong(80)));
        assert_eq!(normalize_handle("@Ada").unwrap(), "ada");
    }
}
