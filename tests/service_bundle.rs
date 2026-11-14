// Info.plist, ServiceHost.m and build.sh must agree, or AppKit silently drops the
// service request and the pasteboard comes back unchanged.

use plist::{Dictionary, Value};
use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn info_plist() -> Dictionary {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("macos/Info.plist");
    Value::from_file(&path)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
        .into_dictionary()
        .expect("Info.plist root must be a dict")
}

fn string<'a>(dict: &'a Dictionary, key: &str) -> &'a str {
    dict.get(key)
        .and_then(Value::as_string)
        .unwrap_or_else(|| panic!("missing string key {key}"))
}

fn strings(dict: &Dictionary, key: &str) -> Vec<String> {
    dict.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing array key {key}"))
        .iter()
        .map(|v| v.as_string().expect("array of strings").to_string())
        .collect()
}

fn proofread_service() -> Dictionary {
    let plist = info_plist();
    let services = plist
        .get("NSServices")
        .and_then(Value::as_array)
        .expect("NSServices array");
    services
        .iter()
        .filter_map(Value::as_dictionary)
        .find(|s| {
            s.get("NSMenuItem")
                .and_then(Value::as_dictionary)
                .map(|m| string(m, "default") == "Proofread Selection")
                .unwrap_or(false)
        })
        .cloned()
        .expect("a service with menu item \"Proofread Selection\"")
}

// Text between the first `@"` after `anchor` and the closing quote.
fn objc_string_after<'a>(source: &'a str, anchor: &str) -> &'a str {
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("{anchor} not found in ServiceHost.m"));
    let rest = &source[start..];
    let open = rest.find("@\"").expect("ObjC string literal") + 2;
    let close = rest[open..].find('"').expect("closing quote") + open;
    &rest[open..close]
}

#[test]
fn ns_message_is_bare_selector_name() {
    let service = proofread_service();
    let message = string(&service, "NSMessage");
    assert!(
        !message.contains(':'),
        "NSMessage {message:?} must not contain ':'; AppKit appends \
         \":userData:error:\" itself, so it would look for {message}:userData:error:"
    );
}

#[test]
fn service_host_implements_ns_message_selector() {
    let service = proofread_service();
    let message = string(&service, "NSMessage");
    let source = read("macos/ServiceHost.m");
    let signature = format!(
        "- (void){message}:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {{"
    );
    assert!(
        source.contains(&signature),
        "ServiceHost.m has no implementation of {message}:userData:error: \
         (expected `{signature}`)"
    );
}

#[test]
fn port_name_matches_registered_provider() {
    let service = proofread_service();
    let port = string(&service, "NSPortName");
    let source = read("macos/ServiceHost.m");
    let registered = objc_string_after(&source, "NSRegisterServicesProvider(");
    assert_eq!(
        port, registered,
        "NSPortName vs NSRegisterServicesProvider name"
    );
}

#[test]
fn service_sends_and_returns_plain_text() {
    let service = proofread_service();
    for key in ["NSSendTypes", "NSReturnTypes"] {
        let types = strings(&service, key);
        assert!(
            types.iter().any(|t| t == "NSStringPboardType"),
            "{key} must include NSStringPboardType, got {types:?}"
        );
    }
}

#[test]
fn bundle_executable_matches_build_output() {
    let plist = info_plist();
    let executable = string(&plist, "CFBundleExecutable");
    let build = read("macos/build.sh");
    assert!(
        build.contains(&format!("$BIN_DIR/{executable}\"")),
        "build.sh must compile to Contents/MacOS/{executable}"
    );
    assert_eq!(string(&plist, "CFBundlePackageType"), "APPL");
}

#[test]
fn service_host_runs_the_bundled_cli() {
    let build = read("macos/build.sh");
    let source = read("macos/ServiceHost.m");
    let looked_up = objc_string_after(&source, "pathForAuxiliaryExecutable:");
    assert!(
        build.contains(&format!("\"$BIN_DIR/{looked_up}\"")),
        "build.sh must copy the CLI to Contents/MacOS/{looked_up}, where ServiceHost.m looks for it"
    );
}
