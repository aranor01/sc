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

/// Compiled content-search matcher: literal substring or regex, optionally
/// anchored to word boundaries. Built once per search/highlight invocation and
/// reused across every line/file it's applied to.
#[derive(Debug)]
pub enum ContentMatcher {
    Literal { needle: String, case_sensitive: bool, whole_words: bool },
    Regex(regex::Regex),
}

/// ASCII alphanumeric or `_`, matching `find_matches`' ASCII-only case-fold semantics.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl ContentMatcher {
    /// Builds a matcher for `needle`. When `is_regex` is set, `whole_words` wraps
    /// the pattern in `\b(?:...)\b` before compiling rather than filtering matches
    /// after the fact, since the `regex` crate's `\b` is already a word-boundary
    /// assertion. Returns `Err(description)` on an invalid regex.
    pub fn build(
        needle: &str,
        is_regex: bool,
        case_sensitive: bool,
        whole_words: bool,
    ) -> Result<Self, String> {
        if is_regex {
            let source = if whole_words {
                format!(r"\b(?:{needle})\b")
            } else {
                needle.to_string()
            };
            let mut builder = regex::RegexBuilder::new(&source);
            builder.case_insensitive(!case_sensitive);
            match builder.build() {
                Ok(r) => Ok(ContentMatcher::Regex(r)),
                Err(e) => Err(format!("Invalid regex: {e}")),
            }
        } else {
            Ok(ContentMatcher::Literal {
                needle: needle.to_string(),
                case_sensitive,
                whole_words,
            })
        }
    }

    /// Byte ranges of all matches in `haystack`.
    pub fn find_matches(&self, haystack: &str) -> Vec<(usize, usize)> {
        match self {
            ContentMatcher::Regex(re) => {
                re.find_iter(haystack).map(|m| (m.start(), m.end())).collect()
            }
            ContentMatcher::Literal { needle, case_sensitive, whole_words } => {
                let raw = find_matches(haystack, needle, *case_sensitive);
                if *whole_words {
                    raw.into_iter()
                        .filter(|&(start, end)| is_word_boundary_match(haystack, start, end))
                        .collect()
                } else {
                    raw
                }
            }
        }
    }
}

/// True if the match at `[start, end)` in `haystack` is bounded by non-word
/// characters (or start/end of string) on both sides. `start`/`end` must be
/// valid UTF-8 char boundaries, which `find_matches` guarantees.
fn is_word_boundary_match(haystack: &str, start: usize, end: usize) -> bool {
    let before_ok = haystack[..start].chars().next_back().map(|c| !is_word_char(c)).unwrap_or(true);
    let after_ok = haystack[end..].chars().next().map(|c| !is_word_char(c)).unwrap_or(true);
    before_ok && after_ok
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

    #[test]
    fn content_matcher_literal_matches_parity() {
        let m = ContentMatcher::build("foo", false, true, false).unwrap();
        assert_eq!(m.find_matches("foo Foo foo"), vec![(0, 3), (8, 11)]);
        let m = ContentMatcher::build("foo", false, false, false).unwrap();
        assert_eq!(m.find_matches("foo Foo FOO"), vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn content_matcher_literal_whole_words() {
        let m = ContentMatcher::build("cat", false, true, true).unwrap();
        assert_eq!(m.find_matches("a cat sat"), vec![(2, 5)]);
        assert!(m.find_matches("category").is_empty());
        assert!(m.find_matches("concatenate").is_empty());
        assert_eq!(m.find_matches("cat"), vec![(0, 3)]);
    }

    #[test]
    fn content_matcher_regex_basic() {
        let m = ContentMatcher::build(r"c.t", true, true, false).unwrap();
        assert_eq!(m.find_matches("cat category"), vec![(0, 3), (4, 7)]);
    }

    #[test]
    fn content_matcher_regex_whole_words_wraps_boundary() {
        let m = ContentMatcher::build(r"c.t", true, true, true).unwrap();
        assert_eq!(m.find_matches("a cat scatter category"), vec![(2, 5)]);
    }

    #[test]
    fn content_matcher_invalid_regex_reports_error() {
        let err = ContentMatcher::build("(", true, true, false).unwrap_err();
        assert!(err.starts_with("Invalid regex:"), "unexpected error: {err}");
    }
}
