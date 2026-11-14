#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

use common::HOST_SOURCES;

fn macos_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("macos")
}

fn compile_and_run_objc_test(sources: &[&str], test: &str) {
    let macos = macos_dir();
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
    compile_and_run_objc_test(&["Restyle.m"], "restyle_test");
}

#[test]
fn selection_leaves_out_what_the_app_added() {
    compile_and_run_objc_test(&["Selection.m"], "selection_test");
}

#[test]
fn service_host_sources_compile_without_warnings() {
    let macos = macos_dir();
    let checked = Command::new("clang")
        .args(["-fsyntax-only", "-fobjc-arc", "-Wall", "-Werror"])
        .args(HOST_SOURCES.iter().map(|source| macos.join(source)))
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}
