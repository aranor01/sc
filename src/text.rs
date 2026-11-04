//! UI-independent text helpers shared by the panels, dialogs and the command palette:
//! fixed-width truncation and fuzzy matching.

pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        format!("{:<width$}", s, width = max)
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

pub(crate) fn truncate_path(s: &str, max: usize) -> String {
    let char_count = s.chars().count();

    if char_count <= max {
        return format!("{:<width$}", s, width = max)
    }

    // Determine how many characters to keep on each side
    let keep_total = max - 1; // reserve 1 spot for '~'
    let keep_left = keep_total / 2;
    let keep_right = keep_total - keep_left;

    // Find byte indices safely without breaking UTF-8 characters
    let left_end_byte = s.char_indices().nth(keep_left).map_or(0, |(idx, _)| idx);
    let right_start_byte = s
        .char_indices()
        .nth(char_count - keep_right)
        .map_or(s.len(), |(idx, _)| idx);

    format!("{left}~{right}", left = &s[..left_end_byte], right = &s[right_start_byte..])
}

/// Truncates from the front with a leading `...`, keeping the tail (the most specific,
/// currently-relevant part of a path) visible instead of the start.
pub(crate) fn truncate_path_front(s: &str, max: usize) -> String {
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    const ELLIPSIS: &str = "...";
    if max <= ELLIPSIS.chars().count() {
        return s.chars().skip(len - max).collect();
    }
    let keep = max - ELLIPSIS.chars().count();
    let tail: String = s.chars().skip(len - keep).collect();
    format!("{ELLIPSIS}{tail}")
}

/// Subsequence match of `query` against `text`, case-insensitive. Higher is better:
/// consecutive characters and word starts score extra, gaps cost. `None` = no match.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let mut qi = 0;
    let mut score = 0;
    let mut prev_match: Option<usize> = None;
    for (i, &c) in t.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if c.to_lowercase().next() != Some(q[qi]) {
            continue;
        }
        score += 10;
        if prev_match == Some(i.wrapping_sub(1)) {
            score += 15;
        } else if let Some(p) = prev_match {
            score -= (i - p - 1).min(10) as i32;
        }
        if i == 0 || !t[i - 1].is_alphanumeric() {
            score += 12;
        }
        prev_match = Some(i);
        qi += 1;
    }
    (qi == q.len()).then_some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_path_front_fits_unchanged() {
        assert_eq!(truncate_path_front("/tmp/short", 20), "/tmp/short");
    }

    #[test]
    fn truncate_path_front_collapses_with_leading_ellipsis() {
        assert_eq!(truncate_path_front("/home/alice/projects/scd", 15), ".../projects/scd");
    }

    #[test]
    fn truncate_path_front_exact_fit_unchanged() {
        assert_eq!(truncate_path_front("/abcde", 6), "/abcde");
    }

    #[test]
    fn truncate_path_front_keeps_tail_when_too_narrow_for_ellipsis() {
        assert_eq!(truncate_path_front("/home/alice/projects", 2), "ts");
    }

    #[test]
    fn fuzzy_requires_ordered_subsequence() {
        assert!(fuzzy_score("srt", "Sort Panel").is_some());
        assert!(fuzzy_score("tsr", "Sort Panel").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("SORT", "sort panel").is_some());
    }

    #[test]
    fn fuzzy_prefers_consecutive_and_word_start_matches() {
        let tight = fuzzy_score("sort", "Sort Panel").unwrap();
        let loose = fuzzy_score("sort", "Show Octal Remote Tree").unwrap();
        assert!(tight > loose);
        let initials = fuzzy_score("sp", "Sort Panel").unwrap();
        let buried = fuzzy_score("sp", "Unselect Group").unwrap();
        assert!(initials > buried);
    }
}
