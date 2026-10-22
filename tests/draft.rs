use easytranslate::draft::{Draft, Snapshot};
fn snap(target: u64, text: &str, language: &str) -> Snapshot {
    Snapshot {
        target,
        text: text.into(),
        language: language.into(),
    }
}
#[test]
fn stale_result_cannot_replace_a_new_draft() {
    let mut draft = Draft::default();
    assert!(draft.observe(Some(snap(1, "今天付款", "en"))));
    let old = draft.generation;
    draft.observe(Some(snap(1, "今天不要付款", "en")));
    assert!(!draft.accept(old, "Pay today".into()));
    assert!(draft.replacement(&snap(1, "今天不要付款", "en")).is_none());
}
#[test]
fn replacement_requires_same_target_text_and_language() {
    let mut draft = Draft::default();
    draft.observe(Some(snap(7, "你好", "en")));
    assert!(draft.accept(draft.generation, "Hello".into()));
    assert_eq!(draft.replacement(&snap(7, "你好", "en")), Some("Hello"));
    for other in [
        snap(8, "你好", "en"),
        snap(7, "你好呀", "en"),
        snap(7, "你好", "fr"),
    ] {
        assert!(draft.replacement(&other).is_none());
    }
}
#[test]
fn clearing_or_switching_focus_invalidates_pending_translation() {
    let mut draft = Draft::default();
    draft.observe(Some(snap(1, "你好", "en")));
    let old = draft.generation;
    assert!(draft.observe(None));
    assert!(!draft.accept(old, "Hello".into()));
    assert!(draft.translation.is_none());
}
#[test]
fn unchanged_poll_does_not_cancel_translation() {
    let mut draft = Draft::default();
    let value = snap(1, "明天见", "en");
    draft.observe(Some(value.clone()));
    let generation = draft.generation;
    assert!(!draft.observe(Some(value)));
    assert_eq!(draft.generation, generation);
    assert!(draft.accept(generation, "See you tomorrow".into()));
}
#[test]
fn empty_results_and_blank_drafts_cannot_be_inserted() {
    let mut draft = Draft::default();
    draft.observe(Some(snap(1, "你好", "en")));
    assert!(!draft.accept(draft.generation, "  \n".into()));
    draft.observe(Some(snap(1, " ", "en")));
    assert!(!draft.accept(draft.generation, "Hello".into()));
}
