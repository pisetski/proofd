// The service host's Objective-C helpers are checked by small programs in macos/tests,
// built and run here so `cargo test` and CI cover them.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

fn compile_and_run(sources: &[&str], test: &str) {
    let macos = Path::new(env!("CARGO_MANIFEST_DIR")).join("macos");
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join(test);

    let compiled = Command::new("clang")
        .args(["-fobjc-arc", "-framework", "AppKit", "-Wall", "-Werror"])
        .args(sources.iter().map(|source| macos.join(source)))
        .arg(macos.join("tests").join(format!("{test}.m")))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "clang failed:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn restyle_keeps_formatting_through_fixes() {
    compile_and_run(&["Restyle.m"], "restyle_test");
}

#[test]
fn selection_leaves_out_what_the_app_added() {
    compile_and_run(&["Selection.m"], "selection_test");
}
