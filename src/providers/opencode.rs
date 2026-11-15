use super::cli_process::{CliInvocation, cwd_without_project_context, run_cli};
use super::cli_resolver::resolve_cli;
use super::opencode_sessions::ForgetSessionsOnDrop;
use super::{Provider, ProviderError};
use std::path::Path;

const NAME: &str = "opencode";
const LOCKED_DOWN_AGENT: &str = "proofd";
// opencode retries a rejected request (e.g. a spent quota) silently; only its error log shows it.
const PRINT_ERROR_LOGS_TO_STDERR: [&str; 3] = ["--print-logs", "--log-level", "ERROR"];

#[derive(Debug, Clone)]
pub struct OpencodeCli {
    pub model: String,
    pub bin: Option<String>,
}

impl Provider for OpencodeCli {
    async fn complete(&self, system: &str, user: &str) -> Result<String, ProviderError> {
        let program = resolve_cli(self.bin.as_deref(), NAME)?;
        let sandbox = cwd_without_project_context();
        let _forget_sessions = sandbox
            .as_deref()
            .map(|dir| ForgetSessionsOnDrop::new(&program, dir));
        let config = tool_less_agent_config(system);
        let stdout = run_cli(CliInvocation {
            name: NAME,
            program: &program,
            args: &run_args(&self.model, sandbox.as_deref()),
            env: &isolated_env(&config),
            stdin_text: user,
            cwd: sandbox.as_deref(),
            fatal_stderr_line: rejected_request_reason,
        })
        .await?;
        Ok(reply_text_from_events(&stdout))
    }
}

fn run_args(model: &str, sandbox: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = [
        "run",
        "--pure",
        "--agent",
        LOCKED_DOWN_AGENT,
        "--model",
        model,
        "--format",
        "json",
    ]
    .into_iter()
    .chain(PRINT_ERROR_LOGS_TO_STDERR)
    .map(String::from)
    .collect();
    if let Some(dir) = sandbox {
        args.extend(["--dir".to_string(), dir.display().to_string()]);
    }
    args
}

// The selection is untrusted: text saying "run this command" must have no tool to run it with.
fn tool_less_agent_config(system: &str) -> String {
    serde_json::json!({
        "agent": {
            LOCKED_DOWN_AGENT: {
                "mode": "primary",
                "prompt": system,
                "permission": { "*": "deny" },
            },
            "title": { "disable": true },
        },
        "share": "disabled",
        "autoupdate": false,
    })
    .to_string()
}

fn isolated_env(config: &str) -> [(&str, &str); 6] {
    [
        ("OPENCODE_CONFIG_CONTENT", config),
        ("OPENCODE_DISABLE_PROJECT_CONFIG", "1"),
        ("OPENCODE_DISABLE_CLAUDE_CODE", "1"),
        ("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1"),
        ("OPENCODE_DISABLE_SHARE", "1"),
        ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
    ]
}

fn rejected_request_reason(error_log_line: &str) -> Option<String> {
    if !error_log_line.contains(r#"message="stream error""#) {
        return None;
    }
    let reason = match error_log_line.split_once(r#"error.error=""#) {
        Some((_, quoted)) => quoted.trim_end().trim_end_matches('"'),
        None => error_log_line.trim(),
    };
    Some(reason.to_string())
}

fn reply_text_from_events(json_events: &str) -> String {
    json_events
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["type"] == "text")
        .filter_map(|event| event["part"]["text"].as_str().map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_select_the_locked_down_agent_and_json_output() {
        let args = run_args("opencode/m", Some(Path::new("/sandbox")));
        let has_pair = |flag: &str, value: &str| {
            args.windows(2)
                .any(|pair| pair[0] == flag && pair[1] == value)
        };
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--pure".to_string()));
        assert!(has_pair("--agent", LOCKED_DOWN_AGENT));
        assert!(has_pair("--model", "opencode/m"));
        assert!(has_pair("--format", "json"));
        assert!(has_pair("--log-level", "ERROR"));
        assert!(has_pair("--dir", "/sandbox"));
    }

    #[test]
    fn config_denies_every_tool_and_skips_title_generation() {
        let config: serde_json::Value =
            serde_json::from_str(&tool_less_agent_config("SYSTEM")).unwrap();
        let agent = &config["agent"][LOCKED_DOWN_AGENT];
        assert_eq!(agent["prompt"], "SYSTEM");
        assert_eq!(agent["permission"]["*"], "deny");
        assert_eq!(config["agent"]["title"]["disable"], true);
        assert_eq!(config["share"], "disabled");
    }

    #[test]
    fn rejected_request_reason_comes_from_opencodes_stream_error_log() {
        let line = r#"timestamp=2026-11-14T15:17:58.057Z level=ERROR run=bf2446a5 message="stream error" providerID=opencode agent=proofd error.error="AI_APICallError: Free usage limit reached. Subscribe: https://opencode.ai/go"
"#;
        assert_eq!(
            rejected_request_reason(line).as_deref(),
            Some("AI_APICallError: Free usage limit reached. Subscribe: https://opencode.ai/go")
        );
        assert_eq!(
            rejected_request_reason(r#"level=ERROR message="stream error" code=429"#).as_deref(),
            Some(r#"level=ERROR message="stream error" code=429"#)
        );
        assert_eq!(
            rejected_request_reason(r#"level=ERROR message="plugin failed""#),
            None
        );
    }

    #[test]
    fn reply_text_joins_text_parts_and_ignores_other_events() {
        let events = [
            r#"{"type":"step_start","sessionID":"ses_1","part":{"type":"step-start"}}"#,
            r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"Hello, "}}"#,
            "not json",
            r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"world."}}"#,
            r#"{"type":"step_finish","sessionID":"ses_1","part":{"type":"step-finish"}}"#,
        ]
        .join("\n");
        assert_eq!(reply_text_from_events(&events), "Hello, world.");
    }
}
