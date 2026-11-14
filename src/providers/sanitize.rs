use super::ProviderError;

pub fn sanitize_output(raw: &str) -> Result<String, ProviderError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ProviderError::EmptyOutput);
    }
    Ok(trimmed.to_string())
}

// Models sometimes wrap their whole answer in a fence. A fenced input keeps its fence,
// and an inner line that would close the opening fence means the output is several blocks.
pub fn strip_wrapping_fence<'a>(output: &'a str, input: &str) -> &'a str {
    let trimmed = output.trim();
    if starts_with_fence(input) {
        return trimmed;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let [first, inner @ .., last] = lines.as_slice() else {
        return trimmed;
    };
    let Some(opening) = fence_marker(first) else {
        return trimmed;
    };
    let closes_opening = |line: &str| fence_marker(line).is_some_and(|m| m.starts_with(opening));
    let last_is_bare_fence = fence_marker(last).is_some_and(|m| m == last.trim());
    if !closes_opening(last) || !last_is_bare_fence || inner.iter().any(|l| closes_opening(l)) {
        return trimmed;
    }
    trimmed[first.len() + 1..trimmed.len() - last.len()].trim()
}

fn starts_with_fence(text: &str) -> bool {
    text.trim_start()
        .lines()
        .next()
        .and_then(fence_marker)
        .is_some()
}

fn fence_marker(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let marker_char = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let marker_len = line.len() - line.trim_start_matches(marker_char).len();
    (marker_len >= 3).then(|| &line[..marker_len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_trims_normal_output() {
        assert_eq!(sanitize_output("  hello world  \n").unwrap(), "hello world");
        assert_eq!(
            sanitize_output("\n\n  multi\n  line  \n\n").unwrap(),
            "multi\n  line"
        );
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(matches!(
            sanitize_output("   \n\t  "),
            Err(ProviderError::EmptyOutput)
        ));
    }

    #[test]
    fn strips_a_fence_the_model_added() {
        assert_eq!(
            strip_wrapping_fence("```\nhello world\n```", "helo"),
            "hello world"
        );
        assert_eq!(
            strip_wrapping_fence("```text\nhello\nworld\n```", "helo"),
            "hello\nworld"
        );
        assert_eq!(strip_wrapping_fence("```\n   \n```", "helo"), "");
    }

    #[test]
    fn keeps_the_fence_of_a_fenced_input() {
        let block = "```rust\nfn main() {}\n```";
        assert_eq!(
            strip_wrapping_fence(block, "```rust\nfn main() {}\n```"),
            block
        );
    }

    #[test]
    fn keeps_fences_of_several_blocks() {
        let blocks = "```js\na()\n```\n\nThen:\n\n```py\nb()\n```";
        assert_eq!(strip_wrapping_fence(blocks, "Text"), blocks);
    }

    #[test]
    fn strips_a_longer_or_tilde_fence_around_inner_fences() {
        let wrapped = "````markdown\nRun:\n```sh\nls\n```\n````";
        assert_eq!(
            strip_wrapping_fence(wrapped, "Run:"),
            "Run:\n```sh\nls\n```"
        );
        assert_eq!(strip_wrapping_fence("~~~\nhello\n~~~", "helo"), "hello");
    }

    #[test]
    fn keeps_a_wrapper_that_does_not_close_its_opening() {
        assert_eq!(
            strip_wrapping_fence("````\nhello\n```", "helo"),
            "````\nhello\n```"
        );
        assert_eq!(
            strip_wrapping_fence("```\nhello\n~~~", "helo"),
            "```\nhello\n~~~"
        );
    }

    #[test]
    fn keeps_the_fence_of_an_input_fenced_with_tildes_or_crlf() {
        let block = "~~~\ncode\n~~~";
        assert_eq!(strip_wrapping_fence(block, block), block);
        assert_eq!(
            strip_wrapping_fence("```\r\nhello\r\n```", "helo\r\n"),
            "hello"
        );
    }

    #[test]
    fn keeps_prose_that_mentions_a_fence_later() {
        let prose = "Wrap code in:\n```\nlike this\n```";
        assert_eq!(
            strip_wrapping_fence(prose, "Wrap code in:\n```\nlike this\n```"),
            prose
        );
    }

    #[test]
    fn keeps_partial_fences() {
        let s = "Use ```rust for code";
        assert_eq!(strip_wrapping_fence(s, "Use ```rust for code"), s);
        assert_eq!(strip_wrapping_fence("```\nunclosed", "x"), "```\nunclosed");
    }
}
