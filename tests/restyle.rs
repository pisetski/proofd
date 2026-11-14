// macos/Restyle.m keeps rich-text formatting through a proofread; its checks live in
// macos/tests/restyle_test.m and run here so `cargo test` and CI cover them.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

#[test]
fn restyle_keeps_formatting_through_fixes() {
    let macos = Path::new(env!("CARGO_MANIFEST_DIR")).join("macos");
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("restyle_test");

    let compiled = Command::new("clang")
        .args(["-fobjc-arc", "-framework", "AppKit", "-Wall", "-Werror"])
        .arg(macos.join("Restyle.m"))
        .arg(macos.join("tests/restyle_test.m"))
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
