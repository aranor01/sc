/// State of a single panel needed for macro expansion.
#[derive(Debug, Clone, Default)]
pub struct PanelContext {
    pub current_file: String,
    pub dir: String,
    pub tagged: Vec<String>,
}

/// Combined context for macro expansion (active + inactive panel).
#[derive(Debug, Clone, Default)]
pub struct MacroContext {
    pub active: PanelContext,
    pub inactive: PanelContext,
}

/// Result of expanding a macro template string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandResult {
    pub text: String,
    /// `true` if `%u` was consumed — caller should clear active panel tags.
    pub untag_active: bool,
    /// `true` if `%U` was consumed — caller should clear inactive panel tags.
    pub untag_inactive: bool,
}

/// Expand macro placeholders in `template` using the given panel context.
///
/// Supported macros (from MacroSubstitution.md):
/// `%f %x %b %d %F %D %t %T %u %U %s %S %%`
/// Unknown sequences (e.g. `%z`) are left as-is.
pub fn expand(template: &str, ctx: &MacroContext) -> ExpandResult {
    let mut text = String::with_capacity(template.len() * 2);
    let mut untag_active = false;
    let mut untag_inactive = false;
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('f') => text.push_str(&shell_escape(&ctx.active.current_file)),
            Some('x') => text.push_str(&shell_escape(file_extension(&ctx.active.current_file))),
            Some('b') => text.push_str(&shell_escape(file_basename(&ctx.active.current_file))),
            Some('d') => text.push_str(&shell_escape(&ctx.active.dir)),
            Some('F') => text.push_str(&shell_escape(&ctx.inactive.current_file)),
            Some('D') => text.push_str(&shell_escape(&ctx.inactive.dir)),
            Some('t') => text.push_str(&join_tagged(&ctx.active.tagged)),
            Some('T') => text.push_str(&join_tagged(&ctx.inactive.tagged)),
            Some('u') => {
                text.push_str(&join_tagged(&ctx.active.tagged));
                untag_active = true;
            }
            Some('U') => {
                text.push_str(&join_tagged(&ctx.inactive.tagged));
                untag_inactive = true;
            }
            Some('s') => {
                if ctx.active.tagged.is_empty() {
                    text.push_str(&ctx.active.current_file);
                } else {
                    text.push_str(&join_tagged(&ctx.active.tagged));
                }
            }
            Some('S') => {
                if ctx.inactive.tagged.is_empty() {
                    text.push_str(&ctx.inactive.current_file);
                } else {
                    text.push_str(&join_tagged(&ctx.inactive.tagged));
                }
            }
            Some('%') => text.push('%'),
            Some(other) => {
                // Unknown macro — leave as-is.
                text.push('%');
                text.push(other);
            }
            None => text.push('%'),
        }
    }

    ExpandResult { text, untag_active, untag_inactive }
}

fn file_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(pos) if pos > 0 => &name[pos + 1..],
        _ => "",
    }
}

fn file_basename(name: &str) -> &str {
    match name.rfind('.') {
        Some(pos) if pos > 0 => &name[..pos],
        _ => name,
    }
}

fn join_tagged(tagged: &[String]) -> String {
    tagged.iter().map(|s| shell_escape(s)).collect::<Vec<_>>().join(" ")
}

/// Backslash-escape shell-special characters in a filename or path.
pub(crate) fn shell_escape(s: &str) -> String {
    if s.chars().all(|c| {
        matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_' | '/' | '+' | ',' | ':' | '@')
    }) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_' | '/' | '+' | ',' | ':' | '@') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        active_file: &str,
        active_dir: &str,
        active_tagged: &[&str],
        inactive_file: &str,
        inactive_dir: &str,
        inactive_tagged: &[&str],
    ) -> MacroContext {
        MacroContext {
            active: PanelContext {
                current_file: active_file.to_string(),
                dir: active_dir.to_string(),
                tagged: active_tagged.iter().map(|s| s.to_string()).collect(),
            },
            inactive: PanelContext {
                current_file: inactive_file.to_string(),
                dir: inactive_dir.to_string(),
                tagged: inactive_tagged.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    fn simple(active_file: &str) -> MacroContext {
        ctx(active_file, "/home", &[], "other.txt", "/tmp", &[])
    }

    #[test]
    fn expand_filename() {
        let r = expand("%f", &simple("foo.txt"));
        assert_eq!(r.text, "foo.txt");
    }

    #[test]
    fn expand_extension() {
        assert_eq!(expand("%x", &simple("foo.tar.gz")).text, "gz");
        assert_eq!(expand("%x", &simple("noext")).text, "");
        assert_eq!(expand("%x", &simple(".hidden")).text, "");
        assert_eq!(expand("%x", &simple("a.")).text, "");
    }

    #[test]
    fn expand_basename() {
        assert_eq!(expand("%b", &simple("foo.tar.gz")).text, "foo.tar");
        assert_eq!(expand("%b", &simple("noext")).text, "noext");
        assert_eq!(expand("%b", &simple(".hidden")).text, ".hidden");
        assert_eq!(expand("%b", &simple("a.")).text, "a");
    }

    #[test]
    fn expand_dir() {
        let r = expand("%d", &ctx("f", "/my/dir", &[], "g", "/other", &[]));
        assert_eq!(r.text, "/my/dir");
    }

    #[test]
    fn expand_inactive_file() {
        let r = expand("%F", &ctx("a.txt", "/x", &[], "b.txt", "/y", &[]));
        assert_eq!(r.text, "b.txt");
    }

    #[test]
    fn expand_inactive_dir() {
        let r = expand("%D", &ctx("a", "/x", &[], "b", "/y/z", &[]));
        assert_eq!(r.text, "/y/z");
    }

    #[test]
    fn expand_tagged() {
        let c = ctx("f", "/d", &["a.txt", "b c.txt"], "g", "/e", &[]);
        let r = expand("%t", &c);
        assert_eq!(r.text, r"a.txt b\ c.txt");
    }

    #[test]
    fn expand_tagged_empty() {
        let c = ctx("f", "/d", &[], "g", "/e", &[]);
        assert_eq!(expand("%t", &c).text, "");
    }

    #[test]
    fn expand_s_no_tagged_falls_back_to_file() {
        let c = ctx("foo.txt", "/d", &[], "g", "/e", &[]);
        assert_eq!(expand("%s", &c).text, "foo.txt");
    }

    #[test]
    fn expand_s_with_tagged_uses_tagged() {
        let c = ctx("foo.txt", "/d", &["a.txt", "b.txt"], "g", "/e", &[]);
        assert_eq!(expand("%s", &c).text, "a.txt b.txt");
    }

    #[test]
    fn expand_big_s_inactive() {
        let c = ctx("a", "/d", &[], "bar.txt", "/e", &[]);
        assert_eq!(expand("%S", &c).text, "bar.txt");

        let c2 = ctx("a", "/d", &[], "bar.txt", "/e", &["x.txt", "y.txt"]);
        assert_eq!(expand("%S", &c2).text, "x.txt y.txt");
    }

    #[test]
    fn expand_u_sets_untag_flag() {
        let c = ctx("f", "/d", &["a.txt"], "g", "/e", &[]);
        let r = expand("%u", &c);
        assert_eq!(r.text, "a.txt");
        assert!(r.untag_active);
        assert!(!r.untag_inactive);
    }

    #[test]
    fn expand_big_u_sets_untag_inactive_flag() {
        let c = ctx("f", "/d", &[], "g", "/e", &["b.txt"]);
        let r = expand("%U", &c);
        assert_eq!(r.text, "b.txt");
        assert!(!r.untag_active);
        assert!(r.untag_inactive);
    }

    #[test]
    fn expand_multiple_macros() {
        let c = ctx("src.txt", "/home", &[], "dst.txt", "/tmp", &[]);
        let r = expand("diff %f %F", &c);
        assert_eq!(r.text, "diff src.txt dst.txt");
    }

    #[test]
    fn expand_percent_escape() {
        assert_eq!(expand("100%%", &simple("f")).text, "100%");
        assert_eq!(expand("%%f", &simple("foo.txt")).text, "%f");
    }

    #[test]
    fn expand_unknown_macro_left_as_is() {
        assert_eq!(expand("%z", &simple("f")).text, "%z");
        assert_eq!(expand("%9", &simple("f")).text, "%9");
    }

    #[test]
    fn expand_trailing_percent() {
        assert_eq!(expand("hello%", &simple("f")).text, "hello%");
    }

    #[test]
    fn shell_escape_safe_name() {
        assert_eq!(shell_escape("foo.txt"), "foo.txt");
        assert_eq!(shell_escape("my-file_2.rs"), "my-file_2.rs");
    }

    #[test]
    fn shell_escape_name_with_space() {
        assert_eq!(shell_escape("my file.txt"), r"my\ file.txt");
    }

    #[test]
    fn shell_escape_name_with_single_quote() {
        assert_eq!(shell_escape("it's"), r"it\'s");
    }

    #[test]
    fn expand_filename_with_space() {
        assert_eq!(expand("%f", &simple("my file.txt")).text, r"my\ file.txt");
    }
}
