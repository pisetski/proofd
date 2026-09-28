use std::path::PathBuf;

/// Embedded base skill. Override with `proofread.md` in the config dir if present.
/// Searched as `$XDG_CONFIG_HOME/proofd/proofread.md`, then
/// `~/.config/proofd/proofread.md`, then the platform-native config dir
/// (`~/Library/Application Support/proofd/proofread.md` on macOS).
const EMBEDDED_SKILL: &str = include_str!("../skills/proofread.md");

const REVISED_ONLY_CONSTRAINTS: &str = r#"Hard constraints (must follow exactly):
Return ONLY the polished text.
Plain text only.
No Markdown fences unless they were present in the input and belong to the text.
No quotes around the answer.
No bullets unless they are part of the revised text.
No explanations.
No analysis sections.
Preserve the original language.
Preserve Markdown, code blocks, indentation, and line breaks.
If the text is already clean, return the input verbatim."#;

/// Load the proofreading skill text, preferring the user override.
pub fn load_skill() -> String {
    for path in skill_candidate_paths() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
    }
    EMBEDDED_SKILL.to_string()
}

pub fn skill_candidate_paths() -> Vec<PathBuf> {
    crate::config::candidate_paths("proofread.md")
}

pub fn skill_override_path() -> Option<PathBuf> {
    let candidates = skill_candidate_paths();
    candidates
        .iter()
        .find(|p| p.exists())
        .or_else(|| candidates.first())
        .cloned()
}

/// Build (system, user) messages with revised-only constraints.
/// User message contains only the text to revise, clearly delimited,
/// preserving input exactly (including multiline / code blocks).
pub fn build_revised_only(input: &str) -> (String, String) {
    let skill = load_skill();
    let system = format!("{skill}\n\n{REVISED_ONLY_CONSTRAINTS}");
    let user = format!("<text_to_revise>\n{input}\n</text_to_revise>");
    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revised_only_constraint_present() {
        let (system, _) = build_revised_only("hello");
        for needle in [
            "Return ONLY the polished text",
            "No explanations",
            "No analysis sections",
            "Preserve the original language",
            "Preserve Markdown, code blocks, indentation, and line breaks",
            "return the input verbatim",
            "Plain text only",
        ] {
            assert!(system.contains(needle), "system missing: {needle}");
        }
    }

    #[test]
    fn input_preserved_exactly_in_user_payload() {
        let input = "  Hello  world!\nSecond line\twith tab  ";
        let (_, user) = build_revised_only(input);
        assert!(user.contains(input));
        assert!(user.starts_with("<text_to_revise>\n"));
        assert!(user.ends_with("\n</text_to_revise>"));
    }

    #[test]
    fn multiline_code_block_stays_intact() {
        let input =
            "Here:\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n- bullet\n  indented";
        let (_, user) = build_revised_only(input);
        assert!(user.contains(input));
        // Delimiters must not mangle interior fences.
        assert_eq!(user.matches("```").count(), 2);
    }
}
