// `proofd polish` is the engine behind the macOS Service: the host pastes back
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
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("fake-claude");
        std::fs::write(&stub, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config_dir = dir.path().join("xdg/proofd");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "provider = \"claude\"\nmodel = \"haiku\"\nprovider_bin = \"{}\"\ntimeout_secs = 10\n",
                stub.display()
            ),
        )
        .unwrap();
        Self { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn polish(&self, input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_proofd"))
            .arg("polish")
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
fn polish_passes_input_to_provider_and_prints_only_revised_text() {
    let sandbox =
        Sandbox::with_provider_script("cat > \"$HOME/stdin.txt\"\necho 'This is a sentence.'");

    let out = sandbox.polish("this are a sentense");

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "This is a sentence.");
    let received = std::fs::read_to_string(sandbox.path("stdin.txt")).unwrap();
    assert!(
        received.contains("this are a sentense"),
        "provider never received the selection: {received:?}"
    );
}

#[test]
fn polish_logs_go_to_stderr_not_stdout() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho 'Fixed.'");

    let out = sandbox.polish("fixd");

    let stderr = text(&out.stderr);
    assert!(stderr.contains("polish start"), "stderr: {stderr}");
    assert!(stderr.contains("polish ok"), "stderr: {stderr}");
    assert!(
        !stderr.contains('\u{1b}'),
        "ANSI escapes in non-terminal stderr: {stderr:?}"
    );
    assert_eq!(text(&out.stdout), "Fixed.");
}

#[test]
fn polish_provider_failure_exits_nonzero_with_empty_stdout() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho 'key expired' >&2\nexit 1");

    let out = sandbox.polish("some text");

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "", "nothing may be pasted on failure");
    assert!(
        text(&out.stderr).contains("key expired"),
        "provider error must reach stderr for the service log: {}",
        text(&out.stderr)
    );
}

#[test]
fn polish_empty_output_from_provider_is_an_error() {
    let sandbox = Sandbox::with_provider_script("cat >/dev/null\necho '   '");

    let out = sandbox.polish("some text");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn polish_rejects_blank_input_without_calling_provider() {
    let sandbox = Sandbox::with_provider_script("touch \"$HOME/called\"");

    let out = sandbox.polish("  \n ");

    assert!(!out.status.success());
    assert_eq!(text(&out.stdout), "");
    assert!(!sandbox.path("called").exists());
}
