/// A compiled filter/select-group pattern.
#[derive(Debug, Clone)]
pub struct FilterPattern {
    pub raw: String,
    pub files_only: bool,
    pub case_sensitive: bool,
    pub is_regex: bool,
    regex: Option<regex::Regex>,
    glob: Option<glob::Pattern>,
}

impl FilterPattern {
    /// Returns true if `name` matches the pattern, respecting case sensitivity.
    /// Does NOT apply the `files_only` flag — callers handle that themselves.
    pub fn matches(&self, name: &str) -> bool {
        if self.is_regex {
            self.regex.as_ref().is_some_and(|r| r.is_match(name))
        } else {
            let opts = glob::MatchOptions {
                case_sensitive: self.case_sensitive,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            };
            self.glob.as_ref().is_some_and(|g| g.matches_with(name, opts))
        }
    }
}

/// Build and compile a filter/select pattern with explicit options.
/// Returns `Err(description)` on invalid patterns.
pub fn build_filter_pattern(
    text: &str,
    files_only: bool,
    case_sensitive: bool,
    is_regexp: bool,
) -> Result<FilterPattern, String> {
    if is_regexp {
        let mut builder = regex::RegexBuilder::new(text);
        builder.case_insensitive(!case_sensitive);
        match builder.build() {
            Ok(r) => Ok(FilterPattern {
                raw: text.to_string(),
                files_only,
                case_sensitive,
                is_regex: true,
                regex: Some(r),
                glob: None,
            }),
            Err(e) => Err(format!("Invalid regex: {e}")),
        }
    } else {
        match glob::Pattern::new(text) {
            Ok(g) => Ok(FilterPattern {
                raw: text.to_string(),
                files_only,
                case_sensitive,
                is_regex: false,
                regex: None,
                glob: Some(g),
            }),
            Err(e) => Err(format!("Invalid glob: {e}")),
        }
    }
}

/// Byte ranges of all non-overlapping occurrences of `needle` in `haystack`.
/// Case-insensitive matching folds ASCII letters only (like `grep -i` in the C
/// locale); byte-wise comparison is UTF-8 safe because a continuation byte can
/// never equal an ASCII byte or a lead byte.
pub fn find_matches(haystack: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        return haystack
            .match_indices(needle)
            .map(|(i, m)| (i, i + m.len()))
            .collect();
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n.len() <= h.len() {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            out.push((i, i + n.len()));
            i += n.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_case_sensitive() {
        assert_eq!(find_matches("foo Foo foo", "foo", true), vec![(0, 3), (8, 11)]);
    }

    #[test]
    fn find_matches_case_insensitive() {
        assert_eq!(find_matches("foo Foo FOO", "foo", false), vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn find_matches_non_overlapping() {
        assert_eq!(find_matches("aaa", "aa", true), vec![(0, 2)]);
    }

    #[test]
    fn find_matches_empty_needle_matches_nothing() {
        assert!(find_matches("abc", "", true).is_empty());
    }

    #[test]
    fn find_matches_utf8_haystack() {
        assert_eq!(find_matches("héllo wörld", "wörld", false), vec![(7, 13)]);
    }

    #[test]
    fn glob_pattern_matches() {
        let p = build_filter_pattern("*.rs", false, true, false).unwrap();
        assert!(p.matches("main.rs"));
        assert!(!p.matches("main.rc"));
    }

    #[test]
    fn regex_pattern_case_insensitive() {
        let p = build_filter_pattern("^ma.*", false, false, true).unwrap();
        assert!(p.matches("Main.rs"));
    }

    #[test]
    fn invalid_regex_reports_error() {
        assert!(build_filter_pattern("(", false, true, true).is_err());
    }
}
