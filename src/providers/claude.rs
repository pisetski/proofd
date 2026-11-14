use super::cli_process::{
    CliInvocation, cwd_without_project_context, no_fatal_stderr_lines, run_cli,
};
use super::cli_resolver::resolve_cli;
use super::{Provider, ProviderError};

const NAME: &str = "claude";
const ENV_SKIPPING_STARTUP_TELEMETRY_AND_UPDATE_CHECKS: [(&str, &str); 1] =
    [("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")];

#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub model: String,
    pub bin: Option<String>,
}

impl Provider for ClaudeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let program = resolve_cli(self.bin.as_deref(), NAME)?;
        run_cli(CliInvocation {
            name: NAME,
            program: &program,
            args: &tool_less_print_mode_args(&self.model, system),
            env: &ENV_SKIPPING_STARTUP_TELEMETRY_AND_UPDATE_CHECKS,
            stdin_text: user,
            cwd: cwd_without_project_context().as_deref(),
            fatal_stderr_line: no_fatal_stderr_lines,
        })
        .await
    }
}

// The selection is untrusted: text saying "run this command" must have no tool to run it with.
fn tool_less_print_mode_args(model: &str, system: &str) -> Vec<String> {
    [
        "-p",
        "--model",
        model,
        "--output-format",
        "text",
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--setting-sources",
        "",
        "--system-prompt",
        system,
    ]
    .map(String::from)
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_isolate_the_agent_and_pass_system_prompt_as_flag() {
        let args = tool_less_print_mode_args("haiku", "SYSTEM");
        let has_pair = |flag: &str, value: &str| {
            args.windows(2)
                .any(|pair| pair[0] == flag && pair[1] == value)
        };
        assert!(has_pair("--tools", ""));
        assert!(has_pair("--setting-sources", ""));
        assert!(has_pair("--system-prompt", "SYSTEM"));
        assert!(has_pair("--model", "haiku"));
        for flag in [
            "--strict-mcp-config",
            "--no-session-persistence",
            "--disable-slash-commands",
        ] {
            assert!(args.contains(&flag.to_string()), "missing {flag}");
        }
    }
}
