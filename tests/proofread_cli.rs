// `proofd` is the engine behind the macOS Service: the host pastes back
// whatever it prints on stdout, so stdout must carry the revised text and nothing else.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;
use tempfile::TempDir;

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    // Fake HOME and XDG_CONFIG_HOME so the user's real config and skill never load.
    // The stub inherits HOME, so scripts can leave evidence under "$HOME".
    fn with_provider_script(script: &str) -> Self {
        Self::with_provider_script_and_config(script, "timeout_secs = 10")
    }

    fn with_provider_script_and_config(script: &str, extra_config: &str) -> Self {
        Self::with_cli_stub("claude", "haiku", script, extra_config)
    }

    fn with_cli_stub(provider: &str, model: &str, script: &str, extra_config: &str) -> Self {
        let sandbox = Self::with_config("");
        let stub = sandbox.path(&format!("fake-{provider}"));
        std::fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        sandbox.write_config(&format!(
            "provider = \"{provider}\"\nmodel = \"{model}\"\nprovider_bin = \"{}\"\n{extra_config}\n",
            stub.display()
        ));
        sandbox
    }

    fn with_openai_server(base_url: &str) -> Self {
        Self::with_config(&format!(
            "provider = \"openai-compat\"\nmodel = \"local-model\"\nbase_url = \"{base_url}\"\ntimeout_secs = 10"
        ))
    }

    fn with_config(config: &str) -> Self {
        let sandbox = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(sandbox.path("xdg/proofd")).unwrap();
        sandbox.write_config(config);
        sandbox
    }

    fn write_config(&self, config: &str) {
        std::fs::write(self.path("xdg/proofd/config.toml"), config).unwrap();
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn proofread(&self, input: &str) -> Output {
        self.proofread_with_env(input, &[])
    }

    fn proofread_with_env(&self, input: &str, env: &[(&str, &str)]) -> Output {
        self.spawn_proofread(input, env).wait_with_output().unwrap()
    }

    fn spawn_proofread(&self, input: &str, env: &[(&str, &str)]) -> Child {
        let mut child = Command::new(env!("CARGO_BIN_EXE_proofd"))
            .envs(env.iter().copied())
            .env("HOME", self.dir.path())
            .env("XDG_CONFIG_HOME", self.path("xdg"))
            .current_dir(self.dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn lines(path: PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
}

struct RecordedRequest {
    head: String,
    body: String,
}

// Answers exactly one request, so each test must make one.
fn serve_once(status: &str, body: &str) -> (String, JoinHandle<RecordedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            head.push_str(&line);
        }
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();
        stream.write_all(response.as_bytes()).unwrap();
        RecordedRequest {
            head,
            body: String::from_utf8(body).unwrap(),
        }
    });
    (base_url, handle)
}

#[test]
fn passes_input_to_provider_and_prints_only_revised_text() {
    let sandbox =
        Sandbox::with_provider_script("cat > \"$HOME/stdin.txt\"\necho 'This is a sentence.'");

    let out = sandbox.proofread("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");
    let received = std::fs::read_to_string(sandbox.path("stdin.txt")).unwrap();
    assert!(
        received.contains("this are a sentense"),
        "provider never received the selection: {received:?}"
    );
}

#[test]
fn logs_go_to_stderr_not_stdout() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho 'Fixed.'");

    let out = sandbox.proofread("fixd");

    let stderr = text(&out.stderr);
    assert!(stderr.contains("proofread start"), "stderr: {stderr}");
    assert!(stderr.contains("proofread ok"), "stderr: {stderr}");
    assert!(
        !stderr.contains('\u{1b}'),
        "ANSI escapes in non-terminal stderr: {stderr:?}"
    );
    assert_eq!(text(&out.stdout), "Fixed.");
}

#[test]
fn provider_failure_exits_nonzero_with_empty_stdout() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho 'key expired' >&2\nexit 1");

    let out = sandbox.proofread("some text");

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "", "nothing may be pasted on failure");
    assert!(
        text(&out.stderr).contains("key expired"),
        "provider error must reach stderr for the service log: {}",
        text(&out.stderr)
    );
}

#[test]
fn empty_output_from_provider_is_an_error() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho '   '");

    let out = sandbox.proofread("some text");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn rejects_blank_input_without_calling_provider() {
    let sandbox = Sandbox::with_provider_script("touch \"$HOME/called\"");

    let out = sandbox.proofread("  \n ");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert!(!sandbox.path("called").exists());
}

#[test]
fn unknown_argument_prints_usage_and_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_proofd"))
        .arg("bogus")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("usage: proofd"));
}

#[test]
fn version_flag_prints_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_proofd"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(text(&out.stdout).starts_with("proofd "));
}

#[test]
fn config_error_is_reported_once_in_plain_words() {
    let out = Sandbox::with_config("timeout_secs = 0").proofread("some text");

    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("proofd: invalid config at "),
        "stderr: {stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "stderr: {stderr}");
}

// The stub keeps one session in proofd's sandbox and one of the user's elsewhere.
fn opencode_stub(on_run: &str) -> String {
    format!(
        r#"case "$1" in
run)
  printf '%s\n' "$@" > "$HOME/args.txt"
  printf '%s' "$OPENCODE_CONFIG_CONTENT" > "$HOME/config.json"
  env | grep '^OPENCODE_DISABLE' > "$HOME/env.txt"
  {on_run}
  ;;
session)
  case "$2" in
  list) printf '[{{"id":"ses_proofd","directory":"%s","created":1,"updated":1,"title":"t"}},{{"id":"ses_user","directory":"/Users/me/project","created":1,"updated":1,"title":"t"}}]' "$HOME/Library/Caches/proofd/cli-sandbox" ;;
  delete) echo "$3" >> "$HOME/deleted.txt" ;;
  esac
  ;;
esac"#
    )
}

fn opencode_sandbox(on_run: &str, extra_config: &str) -> Sandbox {
    Sandbox::with_cli_stub(
        "opencode",
        "opencode/some-model",
        &opencode_stub(on_run),
        extra_config,
    )
}

// Deletion runs detached after proofd exits, so it may land a moment later.
fn deleted_sessions(sandbox: &Sandbox) -> Vec<String> {
    let path = sandbox.path("deleted.txt");
    for _ in 0..100 {
        if path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            return lines(path);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Vec::new()
}

#[test]
fn opencode_runs_a_tool_less_agent_and_prints_its_reply() {
    let sandbox = opencode_sandbox(
        r#"cat > "$HOME/stdin.txt"
  echo '{"type":"step_start","sessionID":"ses_proofd","part":{"type":"step-start"}}'
  echo '{"type":"text","sessionID":"ses_proofd","part":{"type":"text","text":"This is a sentence."}}'"#,
        "timeout_secs = 10",
    );

    let out = sandbox.proofread("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");

    let args = lines(sandbox.path("args.txt"));
    assert_eq!(
        args.first().map(String::as_str),
        Some("run"),
        "args: {args:?}"
    );
    assert!(has_pair(&args, "--agent", "proofd"), "args: {args:?}");
    assert!(
        has_pair(&args, "--model", "opencode/some-model"),
        "args: {args:?}"
    );
    assert!(has_pair(&args, "--format", "json"), "args: {args:?}");
    let dir = args
        .iter()
        .skip_while(|arg| *arg != "--dir")
        .nth(1)
        .unwrap_or_else(|| panic!("no --dir in args: {args:?}"));
    assert!(dir.ends_with("proofd/cli-sandbox"), "dir: {dir}");

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(sandbox.path("config.json")).unwrap())
            .unwrap();
    let agent = &config["agent"]["proofd"];
    assert_eq!(agent["permission"]["*"], "deny", "config: {config}");
    let system = agent["prompt"].as_str().unwrap_or_default();
    assert!(
        system.contains("Return ONLY the corrected text."),
        "config: {config}"
    );
    let env = lines(sandbox.path("env.txt"));
    assert!(
        env.contains(&"OPENCODE_DISABLE_PROJECT_CONFIG=1".to_string()),
        "env: {env:?}"
    );
    assert!(
        env.contains(&"OPENCODE_DISABLE_CLAUDE_CODE=1".to_string()),
        "env: {env:?}"
    );

    let stdin = std::fs::read_to_string(sandbox.path("stdin.txt")).unwrap();
    assert_eq!(
        stdin,
        "<text_to_revise>\nthis are a sentense\n</text_to_revise>"
    );
}

#[test]
fn opencode_sessions_in_the_sandbox_are_deleted_after_success_failure_and_timeout() {
    let cases = [
        (
            r#"cat >/dev/null
  echo '{"type":"text","part":{"type":"text","text":"Fixed."}}'"#,
            "timeout_secs = 10",
        ),
        (
            "cat >/dev/null\n  echo 'model not found' >&2\n  exit 1",
            "timeout_secs = 10",
        ),
        ("sleep 5", "timeout_secs = 1"),
    ];

    for (on_run, config) in cases {
        let sandbox = opencode_sandbox(on_run, config);
        let out = sandbox.proofread("some text");
        assert_eq!(
            deleted_sessions(&sandbox),
            vec!["ses_proofd".to_string()],
            "case {on_run:?}, stderr: {}",
            text(&out.stderr)
        );
    }
}

fn eventually(condition: impl Fn() -> bool) -> bool {
    (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        condition()
    })
}

fn send_signal(signal: &str, pid: &str) -> bool {
    Command::new("kill")
        .args([signal, pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

// A failing assertion must not leave the stub's sleep running.
struct KillOnDrop(String);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        send_signal("-KILL", &self.0);
    }
}

// The service host sends SIGTERM at its deadline, when a slow provider is still running.
#[test]
fn sigterm_stops_the_provider_and_still_deletes_its_session() {
    let sandbox = opencode_sandbox(
        "echo $$ > \"$HOME/provider.pid\"\n  exec sleep 30",
        "timeout_secs = 60",
    );
    let pid_file = sandbox.path("provider.pid");
    let read_pid = || std::fs::read_to_string(&pid_file).unwrap_or_default();

    let proofd = sandbox.spawn_proofread("some text", &[]);
    let _proofd_guard = KillOnDrop(proofd.id().to_string());
    assert!(
        eventually(|| !read_pid().trim().is_empty()),
        "provider never started"
    );
    let provider = KillOnDrop(read_pid().trim().to_string());
    send_signal("-TERM", &proofd.id().to_string());

    let out = proofd.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).ends_with("proofd: stopped by SIGTERM\n"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(
        eventually(|| !send_signal("-0", &provider.0)),
        "provider {} outlived proofd",
        provider.0
    );
    assert_eq!(deleted_sessions(&sandbox), vec!["ses_proofd".to_string()]);
}

#[test]
fn opencode_failure_reports_its_stderr() {
    let sandbox = opencode_sandbox(
        "cat >/dev/null\n  echo 'model not found' >&2\n  exit 1",
        "timeout_secs = 10",
    );

    let out = sandbox.proofread("some text");

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).contains("model not found"),
        "stderr: {}",
        text(&out.stderr)
    );
}

#[test]
fn openai_compat_posts_chat_completion_without_credentials() {
    let (base_url, server) = serve_once(
        "200 OK",
        r#"{"choices":[{"message":{"role":"assistant","content":"This is a sentence."}}]}"#,
    );
    let sandbox = Sandbox::with_openai_server(&base_url);

    let out = sandbox.proofread("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");

    let request = server.join().unwrap();
    assert!(
        request.head.starts_with("POST /v1/chat/completions "),
        "head: {}",
        request.head
    );
    assert!(
        !request.head.to_ascii_lowercase().contains("authorization:"),
        "no credentials may be sent: {}",
        request.head
    );
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "local-model");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("this are a sentense"), "user message: {user}");
}

#[test]
fn openai_compat_http_error_reports_server_message() {
    let (base_url, server) = serve_once("500 Internal Server Error", "no model loaded");
    let sandbox = Sandbox::with_openai_server(&base_url);

    let out = sandbox.proofread("some text");
    server.join().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("500"), "stderr: {stderr}");
    assert!(stderr.contains("no model loaded"), "stderr: {stderr}");
}

#[test]
fn openai_compat_reply_without_content_is_an_error() {
    let (base_url, server) = serve_once("200 OK", r#"{"choices":[]}"#);
    let sandbox = Sandbox::with_openai_server(&base_url);

    let out = sandbox.proofread("some text");
    server.join().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
}

// The service host saves proofd's stderr to ~/Library/Logs, and argv is visible to
// every local process, so neither may ever carry the user's text.
#[test]
fn selection_and_result_never_reach_logs_or_argv() {
    let marker = "4921";
    let selection = format!("my pin is {marker}");
    let all_logs = [("RUST_LOG", "trace")];
    let succeeding = Sandbox::with_provider_script(&format!(
        "printf '%s\\n' \"$@\" > \"$HOME/args.txt\"\ncat >/dev/null\necho 'My PIN is {marker}.'"
    ));
    let failing = Sandbox::with_provider_script("cat >/dev/null\nexit 1");

    let ok = succeeding.proofread_with_env(&selection, &all_logs);
    let failed = failing.proofread_with_env(&selection, &all_logs);

    assert!(ok.status.success(), "stderr: {}", text(&ok.stderr));
    assert!(!failed.status.success());
    for out in [&ok, &failed] {
        let stderr = text(&out.stderr);
        assert!(!stderr.contains(marker), "text leaked to logs: {stderr}");
    }
    let args = lines(succeeding.path("args.txt"));
    assert!(
        !args.iter().any(|arg| arg.contains(marker)),
        "text leaked to argv: {args:?}"
    );
}

// The service host turns proofd's last "proofd: " stderr line into a few words for the
// failure pill by prefix (`shortFailureReason` in macos/ServiceHost.m); reworded errors
// must not silently degrade to the generic reason.
fn service_failure_reasons() -> Vec<(String, String)> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("macos/ServiceHost.m"),
    )
    .unwrap();
    let table_start = source.find("*reasons = @[").expect("reason table");
    let table = &source[table_start..];
    let table = &table[..table.find("];").unwrap()];
    table
        .lines()
        .filter_map(|line| {
            let entry = line.trim().strip_prefix("@[@\"")?.strip_suffix("\"],")?;
            let (prefix, reason) = entry.split_once("\", @\"")?;
            Some((prefix.to_string(), reason.to_string()))
        })
        .collect()
}

fn pill_reason(out: &Output) -> String {
    let stderr = text(&out.stderr);
    let message = stderr
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("proofd: "))
        .unwrap_or_else(|| panic!("no `proofd: ` line in stderr: {stderr}"));
    service_failure_reasons()
        .into_iter()
        .find(|(prefix, _)| message.starts_with(prefix.as_str()))
        .map(|(_, reason)| reason)
        .unwrap_or_else(|| "Proofread failed".to_string())
}

#[test]
fn service_failure_reasons_match_proofd_errors() {
    let missing_bin = Sandbox::with_config("provider_bin = \"/nonexistent/claude\"");
    let not_executable = Sandbox::with_config("");
    std::fs::write(not_executable.path("claude"), "").unwrap();
    not_executable.write_config(&format!(
        "provider_bin = \"{}\"",
        not_executable.path("claude").display()
    ));
    let (http_error_url, http_error_server) = serve_once("500 Internal Server Error", "boom");
    let unreadable_config = Sandbox::with_config("");
    std::fs::remove_file(unreadable_config.path("xdg/proofd/config.toml")).unwrap();
    std::fs::create_dir(unreadable_config.path("xdg/proofd/config.toml")).unwrap();

    let cases = [
        (
            Sandbox::with_provider_script_and_config("sleep 5", "timeout_secs = 1"),
            "some text",
            "Timed out",
        ),
        (missing_bin, "some text", "Can't start provider"),
        (not_executable, "some text", "Can't start provider"),
        (
            Sandbox::with_config(
                "provider = \"openai-compat\"\nbase_url = \"http://127.0.0.1:1/v1\"\ntimeout_secs = 5",
            ),
            "some text",
            "Can't reach provider",
        ),
        (
            Sandbox::with_openai_server(&http_error_url),
            "some text",
            "Provider error",
        ),
        (
            Sandbox::with_provider_script("cat >/dev/null\necho 'model \"x\" not found' >&2\nexit 1"),
            "some text",
            "Provider error",
        ),
        (Sandbox::with_config("timeout_secs = 0"), "some text", "Check proofd config"),
        (unreadable_config, "some text", "Check proofd config"),
        (Sandbox::with_config("max_input_chars = 3"), "some text", "Selection too long"),
        (Sandbox::with_provider_script("touch \"$HOME/called\""), "  \n ", "No text selected"),
        (
            Sandbox::with_provider_script("cat >/dev/null\necho '   '"),
            "some text",
            "Empty response",
        ),
        (
            Sandbox::with_provider_script("cat >/dev/null\nprintf '```\\n \\n```'"),
            "some text",
            "Empty response",
        ),
    ];

    for (sandbox, input, expected) in cases {
        let out = sandbox.proofread(input);
        assert!(!out.status.success());
        assert_eq!(pill_reason(&out), expected, "stderr: {}", text(&out.stderr));
    }
    http_error_server.join().unwrap();
}
