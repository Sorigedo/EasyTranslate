/// A missing AX response is not evidence that a different control has focus.
/// Retaining a binding never authorizes a write; replacement still rereads AX.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Observe,
    Retry,
    Preview,
    Unbind,
}
pub fn decide(
    bound_pid: i32,
    front_pid: Option<i32>,
    same_control: Option<bool>,
    own_pid: i32,
) -> Decision {
    if front_pid == Some(own_pid) {
        return Decision::Preview;
    }
    if front_pid != Some(bound_pid) {
        return Decision::Unbind;
    }
    match same_control {
        Some(true) => Decision::Observe,
        Some(false) => Decision::Unbind,
        None => Decision::Retry,
    }
}
