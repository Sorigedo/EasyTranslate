use easytranslate::bridge::{Request, decode};
#[test]
fn successful_translation_preserves_unicode_and_newlines() {
    assert_eq!(
        decode(br#"{"text":"Hello\nWorld"}"#)
            .unwrap()
            .text
            .as_deref(),
        Some("Hello\nWorld")
    );
}
#[test]
fn provider_errors_are_not_accepted_as_translation() {
    assert!(decode(br#"{"error":"missing model"}"#).is_err());
    assert!(decode(b"not json").is_err());
    assert!(decode(br#"{"text":"  "}"#).is_err());
    assert!(decode(br#"{}"#).is_err());
}
#[test]
fn request_serialization_keeps_literal_prompt_and_linebreaks() {
    let request = Request {
        action: "translate".into(),
        text: Some("不要付款\n$50".into()),
        source: Some("zh-Hans".into()),
        target: Some("en".into()),
        path: None,
    };
    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["text"], "不要付款\n$50");
    assert_eq!(value["source"], "zh-Hans");
}
