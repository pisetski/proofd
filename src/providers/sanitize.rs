use super::ProviderError;

const SESSION_HEADER_PREFIXES: [&str; 4] = ["Session:", "Thinking:", "model:", "opencode:"];

pub fn sanitize_output(raw: &str) -> Result<String, ProviderError> {
    let unfenced = strip_single_fence(raw.trim()).trim();
    if unfenced.is_empty() {
        return Err(ProviderError::EmptyOutput);
    }
    Ok(unfenced.to_string())
}

fn strip_single_fence(trimmed: &str) -> &str {
    if !trimmed.starts_with("```") {
        return trimmed;
    }
    let Some(opening_fence_end) = trimmed.find('\n') else {
        return trimmed;
    };
    let Some(closing_fence_start) = trimmed.rfind("```") else {
        return trimmed;
    };
    if closing_fence_start <= opening_fence_end {
        return trimmed;
    }
    trimmed[opening_fence_end + 1..closing_fence_start].trim_matches('\n')
}

pub fn strip_session_headers(output: &str) -> String {
    output
        .lines()
        .skip_while(|line| is_session_header(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_session_header(line: &str) -> bool {
    let line = line.trim();
    SESSION_HEADER_PREFIXES
        .iter()
        .any(|prefix| line.starts_with(prefix))
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
    fn sanitize_strips_single_accidental_fence() {
        let fenced = "```\nhello world\n```";
        assert_eq!(sanitize_output(fenced).unwrap(), "hello world");
        let fenced_lang = "```text\nhello\nworld\n```";
        assert_eq!(sanitize_output(fenced_lang).unwrap(), "hello\nworld");
        let inner = "```\n  indented\n\tcode  \n```";
        assert_eq!(sanitize_output(inner).unwrap(), "indented\n\tcode");
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(matches!(
            sanitize_output("   \n\t  "),
            Err(ProviderError::EmptyOutput)
        ));
        assert!(matches!(
            sanitize_output("```\n   \n```"),
            Err(ProviderError::EmptyOutput)
        ));
    }

    #[test]
    fn sanitize_does_not_strip_partial_fences() {
        let s = "Use ```rust for code";
        assert_eq!(sanitize_output(s).unwrap(), "Use ```rust for code");
    }

    #[test]
    fn session_headers_stripped_only_at_start() {
        let output = "Session: abc\nThinking: hmm\nhello\nSession: kept";
        assert_eq!(strip_session_headers(output), "hello\nSession: kept");
    }

    #[test]
    fn output_without_headers_is_unchanged() {
        assert_eq!(strip_session_headers("hello\nworld"), "hello\nworld");
    }
}
