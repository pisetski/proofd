const MOST_CHANGED_FRACTION: f64 = 0.35;
const EDITS_ALWAYS_ALLOWED: usize = 8;

pub fn looks_like_revision_of(selection: &str, revision: &str) -> bool {
    let selection: Vec<char> = selection.trim().chars().collect();
    let revision: Vec<char> = revision.trim().chars().collect();
    let longest = selection.len().max(revision.len());
    let allowed_edits = EDITS_ALWAYS_ALLOWED.max((longest as f64 * MOST_CHANGED_FRACTION) as usize);
    selection.len().abs_diff(revision.len()) <= allowed_edits
        && edit_distance(&selection, &revision) <= allowed_edits
}

pub fn edit_distance(a: &[char], b: &[char]) -> usize {
    let common_prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[common_prefix..], &b[common_prefix..]);
    let common_suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - common_suffix], &b[..b.len() - common_suffix]);

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(x != y);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(a: &str, b: &str) -> usize {
        edit_distance(
            &a.chars().collect::<Vec<_>>(),
            &b.chars().collect::<Vec<_>>(),
        )
    }

    #[test]
    fn distance_counts_inserted_deleted_and_substituted_characters() {
        assert_eq!(distance("helo", "hello"), 1);
        assert_eq!(distance("hello", "helo"), 1);
        assert_eq!(distance("teh", "the"), 2);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("same", "same"), 0);
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(distance("naïve café", "naive cafe"), 2);
    }

    #[test]
    fn corrections_of_short_selections_are_revisions() {
        assert!(looks_like_revision_of("helo", "hello"));
        assert!(looks_like_revision_of("teh", "The"));
        assert!(looks_like_revision_of("i has a apple", "I have an apple."));
    }

    #[test]
    fn whitespace_around_the_selection_does_not_count_as_a_change() {
        assert!(looks_like_revision_of(
            "\n\n\n    helo\n\n\n\n\n\n\n",
            "hello"
        ));
        assert!(looks_like_revision_of(
            "helo",
            "\n\n\n\n\n\n\n\n\n\n\nhello\n"
        ));
    }

    #[test]
    fn a_refusal_is_not_a_revision_of_a_short_selection() {
        let refusal = "I'm sorry, but I can't help with that request.";
        assert!(!looks_like_revision_of("some text", refusal));
        assert!(!looks_like_revision_of("helo wrold", refusal));
    }

    #[test]
    fn a_preface_in_front_of_a_short_selection_is_not_a_revision() {
        assert!(!looks_like_revision_of(
            "helo wrold",
            "Here is the corrected text:\nHello world"
        ));
    }

    #[test]
    fn a_reply_cut_short_or_blown_up_is_not_a_revision() {
        let selection = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let truncated = &selection[..selection.len() / 10];
        assert!(!looks_like_revision_of(&selection, truncated));
        assert!(!looks_like_revision_of(truncated, &selection));
    }

    #[test]
    fn a_reply_missing_more_than_a_third_of_the_selection_is_not_a_revision() {
        let selection = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let two_thirds = &selection[..selection.len() * 2 / 3];
        let three_fifths = &selection[..selection.len() * 3 / 5];
        assert!(looks_like_revision_of(&selection, two_thirds));
        assert!(!looks_like_revision_of(&selection, three_fifths));
    }

    #[test]
    fn a_text_with_a_fifth_of_its_characters_wrong_is_still_a_revision_of_its_fix() {
        let fixed = "Nothing in the plan changed, except that the deadline moved to Friday. ";
        let typos: String = fixed
            .chars()
            .enumerate()
            .map(|(i, c)| if i % 5 == 0 && c != ' ' { 'x' } else { c })
            .collect();
        assert!(looks_like_revision_of(&typos.repeat(10), &fixed.repeat(10)));
    }
}
