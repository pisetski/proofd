// `proofd` is the engine behind the macOS Service: the host pastes back
// whatever it prints on stdout, so stdout must carry the revised text and nothing else.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
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
        let sandbox = Self::with_config("");
        let stub = sandbox.path("fake-claude");
        std::fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        sandbox.write_config(&format!(
            "provider = \"claude\"\nmodel = \"haiku\"\nprovider_bin = \"{}\"\n{extra_config}\n",
            stub.display()
        ));
        sandbox
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_proofd"))
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
        child.wait_with_output().unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
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
        .filter_map(|line| line.strip_prefix("proofd: "))
        .last()
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
    ];

    for (sandbox, input, expected) in cases {
        let out = sandbox.proofread(input);
        assert!(!out.status.success());
        assert_eq!(
            pill_reason(&out),
            expected,
            "stderr: {}",
            text(&out.stderr)
        );
    }
}
