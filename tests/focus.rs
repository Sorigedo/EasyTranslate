use easytranslate::focus::{Decision, decide};
#[test]
fn temporary_unavailable_focus_keeps_binding_only_in_original_app() {
    assert_eq!(decide(12, Some(12), None, 99), Decision::Retry);
    assert_eq!(decide(12, Some(34), None, 99), Decision::Unbind);
    assert_eq!(decide(12, None, None, 99), Decision::Unbind);
}
#[test]
fn different_control_unbinds_even_inside_same_application() {
    assert_eq!(decide(12, Some(12), Some(false), 99), Decision::Unbind);
    assert_eq!(decide(12, Some(12), Some(true), 99), Decision::Observe);
}
#[test]
fn preview_edit_does_not_unbind_chat_or_allow_observation() {
    assert_eq!(decide(12, Some(99), Some(false), 99), Decision::Preview);
}
