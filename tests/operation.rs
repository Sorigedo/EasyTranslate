use easytranslate::operation::Operation;
#[test]
fn older_ocr_is_rejected_after_new_editor_operation() {
    let mut operation = Operation::default();
    let screenshot = operation.begin();
    assert!(operation.accepts(screenshot));
    operation.begin();
    assert!(!operation.accepts(screenshot));
}

#[test]
fn new_operation_cancels_the_previous_workers_token() {
    use std::sync::atomic::Ordering;
    let mut operation = Operation::default();
    let first = operation.begin();
    let (token, expected) = operation.cancellation();
    assert_eq!(expected, first);
    assert_eq!(token.load(Ordering::Relaxed), expected);
    operation.begin();
    assert_ne!(token.load(Ordering::Relaxed), expected);
}
