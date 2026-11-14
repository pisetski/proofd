#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;
use tempfile::TempDir;

struct FakeHome {
    dir: TempDir,
}

impl FakeHome {
    fn with_claude_stub(script: &str) -> Self {
        Self::with_claude_stub_and_config(script, "timeout_secs = 10")
    }

    fn with_claude_stub_and_config(script: &str, extra_config: &str) -> Self {
        Self::with_cli_stub("claude", "haiku", script, extra_config)
    }

    fn with_cli_stub(provider: &str, model: &str, script: &str, extra_config: &str) -> Self {
        let home = Self::with_config("");
        let stub = home.path(&format!("fake-{provider}"));
        std::fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        home.write_config(&format!(
            "provider = \"{provider}\"\nmodel = \"{model}\"\nprovider_bin = \"{}\"\n{extra_config}\n",
            stub.display()
        ));
        home
    }

    fn with_openai_server(base_url: &str) -> Self {
        Self::with_config(&format!(
            "provider = \"openai-compat\"\nmodel = \"local-model\"\nbase_url = \"{base_url}\"\ntimeout_secs = 10"
        ))
    }

    fn with_config(config: &str) -> Self {
        let home = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(home.path("xdg/proofd")).unwrap();
        home.write_config(config);
        home
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

fn serve_one_request(status: &str, body: &str) -> (String, JoinHandle<RecordedRequest>) {
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
    let home = FakeHome::with_claude_stub("cat > \"$HOME/stdin.txt\"\necho 'This is a sentence.'");

    let out = home.proofread("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");
    let received = std::fs::read_to_string(home.path("stdin.txt")).unwrap();
    assert!(
        received.contains("this are a sentense"),
        "provider never received the selection: {received:?}"
    );
}

#[test]
fn logs_go_to_stderr_not_stdout() {
    let home = FakeHome::with_claude_stub("cat >/dev/null\necho 'Fixed.'");

    let out = home.proofread_with_env("fixd", &[("RUST_LOG", "info")]);

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
    let home = FakeHome::with_claude_stub("cat >/dev/null\necho 'key expired' >&2\nexit 1");

    let out = home.proofread("some text");

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
    let home = FakeHome::with_claude_stub("cat >/dev/null\necho '   '");

    let out = home.proofread("some text");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn rejects_blank_input_without_calling_provider() {
    let home = FakeHome::with_claude_stub("touch \"$HOME/called\"");

    let out = home.proofread("  \n ");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert!(!home.path("called").exists());
}

#[test]
fn default_stderr_stays_empty_for_editor_filters() {
    let home = FakeHome::with_claude_stub("cat >/dev/null\necho 'Fixed.'");

    let out = home.proofread("fixd");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
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
    let out = FakeHome::with_config("timeout_secs = 0").proofread("some text");

    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("proofd: invalid config at "),
        "stderr: {stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "stderr: {stderr}");
}

fn opencode_stub_with_proofd_and_user_sessions(on_run: &str) -> String {
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

fn opencode_home(on_run: &str, extra_config: &str) -> FakeHome {
    FakeHome::with_cli_stub(
        "opencode",
        "opencode/some-model",
        &opencode_stub_with_proofd_and_user_sessions(on_run),
        extra_config,
    )
}

fn wait_for_deleted_sessions(home: &FakeHome) -> Vec<String> {
    let path = home.path("deleted.txt");
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
    let home = opencode_home(
        r#"cat > "$HOME/stdin.txt"
  echo '{"type":"step_start","sessionID":"ses_proofd","part":{"type":"step-start"}}'
  echo '{"type":"text","sessionID":"ses_proofd","part":{"type":"text","text":"This is a sentence."}}'"#,
        "timeout_secs = 10",
    );

    let out = home.proofread("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");

    let args = lines(home.path("args.txt"));
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
        serde_json::from_str(&std::fs::read_to_string(home.path("config.json")).unwrap()).unwrap();
    let agent = &config["agent"]["proofd"];
    assert_eq!(agent["permission"]["*"], "deny", "config: {config}");
    let system = agent["prompt"].as_str().unwrap_or_default();
    assert!(
        system.contains("Return ONLY the corrected text."),
        "config: {config}"
    );
    let env = lines(home.path("env.txt"));
    assert!(
        env.contains(&"OPENCODE_DISABLE_PROJECT_CONFIG=1".to_string()),
        "env: {env:?}"
    );
    assert!(
        env.contains(&"OPENCODE_DISABLE_CLAUDE_CODE=1".to_string()),
        "env: {env:?}"
    );

    let stdin = std::fs::read_to_string(home.path("stdin.txt")).unwrap();
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
        let home = opencode_home(on_run, config);
        let out = home.proofread("some text");
        assert_eq!(
            wait_for_deleted_sessions(&home),
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

struct KillProcessOnDrop(String);

impl Drop for KillProcessOnDrop {
    fn drop(&mut self) {
        send_signal("-KILL", &self.0);
    }
}

#[test]
fn host_deadline_sigterm_and_ctrl_c_sigint_stop_the_provider_and_still_delete_its_session() {
    for signal in ["TERM", "INT"] {
        stops_the_provider_and_still_deletes_its_session(signal);
    }
}

fn stops_the_provider_and_still_deletes_its_session(signal: &str) {
    let home = opencode_home(
        "echo $$ > \"$HOME/provider.pid\"\n  exec sleep 30",
        "timeout_secs = 60",
    );
    let pid_file = home.path("provider.pid");
    let read_pid = || std::fs::read_to_string(&pid_file).unwrap_or_default();

    let proofd = home.spawn_proofread("some text", &[]);
    let _proofd_guard = KillProcessOnDrop(proofd.id().to_string());
    assert!(
        eventually(|| !read_pid().trim().is_empty()),
        "provider never started"
    );
    let provider = KillProcessOnDrop(read_pid().trim().to_string());
    send_signal(&format!("-{signal}"), &proofd.id().to_string());

    let out = proofd.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).ends_with(&format!("proofd: stopped by SIG{signal}\n")),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(
        eventually(|| !send_signal("-0", &provider.0)),
        "provider {} outlived proofd",
        provider.0
    );
    assert_eq!(
        wait_for_deleted_sessions(&home),
        vec!["ses_proofd".to_string()]
    );
}

#[test]
fn opencode_rejected_request_fails_at_once_instead_of_retrying_until_timeout() {
    let home = opencode_home(
        r#"cat >/dev/null
  echo 'timestamp=2026-11-14T15:17:58.057Z level=ERROR message="stream error" agent=proofd error.error="AI_APICallError: Free usage limit reached."' >&2
  exec sleep 30"#,
        "timeout_secs = 20",
    );

    let started = std::time::Instant::now();
    let out = home.proofread("some text");

    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "waited {:?} for the timeout",
        started.elapsed()
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let stderr = text(&out.stderr);
    assert!(
        stderr.ends_with("opencode: AI_APICallError: Free usage limit reached.\n"),
        "stderr: {stderr}"
    );
    assert_eq!(failure_pill_shown_for(&out), "Provider error");
    let args = lines(home.path("args.txt"));
    assert!(args.contains(&"--print-logs".to_string()), "args: {args:?}");
    assert!(has_pair(&args, "--log-level", "ERROR"), "args: {args:?}");
}

#[test]
fn opencode_failure_reports_its_stderr() {
    let home = opencode_home(
        "cat >/dev/null\n  echo 'model not found' >&2\n  exit 1",
        "timeout_secs = 10",
    );

    let out = home.proofread("some text");

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
    let (base_url, server) = serve_one_request(
        "200 OK",
        r#"{"choices":[{"message":{"role":"assistant","content":"This is a sentence."}}]}"#,
    );
    let home = FakeHome::with_openai_server(&base_url);

    let out = home.proofread("this are a sentense");

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
    let (base_url, server) = serve_one_request("500 Internal Server Error", "no model loaded");
    let home = FakeHome::with_openai_server(&base_url);

    let out = home.proofread("some text");
    server.join().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("500"), "stderr: {stderr}");
    assert!(stderr.contains("no model loaded"), "stderr: {stderr}");
}

#[test]
fn openai_compat_reply_without_content_is_an_error() {
    let (base_url, server) = serve_one_request("200 OK", r#"{"choices":[]}"#);
    let home = FakeHome::with_openai_server(&base_url);

    let out = home.proofread("some text");
    server.join().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn selection_and_result_never_reach_service_log_or_world_readable_argv() {
    let marker = "4921";
    let selection = format!("my pin is {marker}");
    let all_logs = [("RUST_LOG", "trace")];
    let succeeding = FakeHome::with_claude_stub(&format!(
        "printf '%s\\n' \"$@\" > \"$HOME/args.txt\"\ncat >/dev/null\necho 'My PIN is {marker}.'"
    ));
    let failing = FakeHome::with_claude_stub("cat >/dev/null\nexit 1");

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

fn service_host_failure_pill_table() -> Vec<(String, String)> {
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

fn failure_pill_shown_for(out: &Output) -> String {
    let stderr = text(&out.stderr);
    let message = stderr
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("proofd: "))
        .unwrap_or_else(|| panic!("no `proofd: ` line in stderr: {stderr}"));
    service_host_failure_pill_table()
        .into_iter()
        .find(|(prefix, _)| message.starts_with(prefix.as_str()))
        .map(|(_, reason)| reason)
        .unwrap_or_else(|| "Proofread failed".to_string())
}

#[test]
fn every_proofd_error_gets_its_own_failure_pill() {
    let missing_bin = FakeHome::with_config("provider_bin = \"/nonexistent/claude\"");
    let not_executable = FakeHome::with_config("");
    std::fs::write(not_executable.path("claude"), "").unwrap();
    not_executable.write_config(&format!(
        "provider_bin = \"{}\"",
        not_executable.path("claude").display()
    ));
    let (http_error_url, http_error_server) =
        serve_one_request("500 Internal Server Error", "boom");
    let unreadable_config = FakeHome::with_config("");
    std::fs::remove_file(unreadable_config.path("xdg/proofd/config.toml")).unwrap();
    std::fs::create_dir(unreadable_config.path("xdg/proofd/config.toml")).unwrap();

    let cases = [
        (
            FakeHome::with_claude_stub_and_config("sleep 5", "timeout_secs = 1"),
            "some text",
            "Timed out",
        ),
        (missing_bin, "some text", "Can't start provider"),
        (not_executable, "some text", "Can't start provider"),
        (
            FakeHome::with_config(
                "provider = \"openai-compat\"\nmodel = \"local-model\"\nbase_url = \"http://127.0.0.1:1/v1\"\ntimeout_secs = 5",
            ),
            "some text",
            "Can't reach provider",
        ),
        (
            FakeHome::with_openai_server(&http_error_url),
            "some text",
            "Provider error",
        ),
        (
            FakeHome::with_claude_stub("cat >/dev/null\necho 'model \"x\" not found' >&2\nexit 1"),
            "some text",
            "Provider error",
        ),
        (
            FakeHome::with_config("timeout_secs = 0"),
            "some text",
            "Check proofd config",
        ),
        (unreadable_config, "some text", "Check proofd config"),
        (
            FakeHome::with_config("max_input_chars = 3"),
            "some text",
            "Selection too long",
        ),
        (
            FakeHome::with_claude_stub("touch \"$HOME/called\""),
            "  \n ",
            "No text selected",
        ),
        (
            FakeHome::with_claude_stub("cat >/dev/null\necho '   '"),
            "some text",
            "Empty response",
        ),
        (
            FakeHome::with_claude_stub("cat >/dev/null\nprintf '```\\n \\n```'"),
            "some text",
            "Empty response",
        ),
    ];

    for (home, input, expected) in cases {
        let out = home.proofread(input);
        assert!(!out.status.success());
        assert_eq!(
            failure_pill_shown_for(&out),
            expected,
            "stderr: {}",
            text(&out.stderr)
        );
    }
    http_error_server.join().unwrap();
}
