use proofd_lib::prompt::build_revised_only;

#[test]
fn revised_only_constraint_is_present() {
    let (system, _) = build_revised_only("hello");
    assert!(system.contains("Return ONLY the polished text"));
    assert!(system.contains("No explanations"));
    assert!(system.contains("Preserve the original language"));
}

#[test]
fn input_is_preserved_exactly_in_user_payload() {
    let input = "line one\n  indented line\n```code\nx\n```";
    let (_, user) = build_revised_only(input);
    assert!(user.contains(input));
}

#[test]
fn multiline_code_block_stays_intact() {
    let input = "Fix:\n```rust\nfn f() {}\n```\n- item\n  - nested";
    let (_, user) = build_revised_only(input);
    assert!(user.contains(input));
    assert_eq!(user.matches("```").count(), 2);
}
