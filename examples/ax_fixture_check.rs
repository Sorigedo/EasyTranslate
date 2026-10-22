// Run only against the dedicated, disposable fixture launched by scripts/test-ax.sh.
#[path = "../src/platform/accessibility.rs"]
#[allow(dead_code)]
mod accessibility;
fn main() {
    let expected_pid: i32 = std::env::args()
        .nth(1)
        .expect("fixture pid")
        .parse()
        .unwrap();
    let original = accessibility::focused().expect("fixture exposes focused editor");
    assert_eq!(
        original.pid, expected_pid,
        "Refusing to modify any other application"
    );
    assert_eq!(original.text, "这是自动测试草稿，不要发送。");
    accessibility::replace(&original, "This is a test draft. Do not send.")
        .expect("replace fixture");
    let updated = accessibility::focused().unwrap();
    assert_eq!(updated.text, "This is a test draft. Do not send.");
    assert!(
        accessibility::replace(&original, "STALE").is_err(),
        "stale draft must be rejected"
    );
    assert_eq!(accessibility::focused().unwrap().text, updated.text);
    println!("PASS: real AX focused read, replacement, stale-write refusal");
}
