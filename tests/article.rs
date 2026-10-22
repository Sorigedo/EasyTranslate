use easytranslate::article::{Part, split};
#[test]
fn preserves_paragraph_order_and_exact_blank_lines() {
    assert_eq!(
        split("你好\n\n  明天见\r\n", 100),
        vec![
            Part::Translate("你好".into()),
            Part::Whitespace("\n\n  ".into()),
            Part::Translate("明天见".into()),
            Part::Whitespace("\r\n".into())
        ]
    );
}
#[test]
fn splits_unicode_without_losing_characters_or_exceeding_limit() {
    let parts = split("你好😀世界再见", 3);
    assert_eq!(
        parts,
        vec![
            Part::Translate("你好😀".into()),
            Part::Translate("世界再".into()),
            Part::Translate("见".into())
        ]
    );
}
#[test]
fn keeps_sentence_words_together_when_possible() {
    assert_eq!(
        split("Hello world again", 12),
        vec![
            Part::Translate("Hello world".into()),
            Part::Whitespace(" ".into()),
            Part::Translate("again".into())
        ]
    );
}

#[test]
fn online_preserves_exact_bytes_and_each_paragraph_under_service_limit() {
    use easytranslate::article::split_online;
    for text in [
        "a".repeat(501),
        "中".repeat(334),
        "😀".repeat(251),
        format!("  {}\r\n\r\n{}\n", "A中😀 ".repeat(100), "b".repeat(500)),
    ] {
        let parts = split_online(&text);
        let mut restored = String::new();
        for part in parts {
            match part {
                Part::Translate(value) => {
                    assert!(!value.is_empty() && value.len() <= 500);
                    assert!(!value.contains(['\r', '\n']));
                    restored.push_str(&value);
                }
                Part::Whitespace(value) => restored.push_str(&value),
            }
        }
        assert_eq!(restored, text);
    }
}

#[test]
fn llm_groups_short_multiline_text_into_one_contextual_request() {
    let text = "锄禾日当午，\n汗滴禾下土，\n谁知盘中餐，\n粒粒皆辛苦。";
    assert_eq!(
        easytranslate::article::split_llm(text, 12000),
        vec![easytranslate::article::Part::Translate(text.into())]
    );
}

#[test]
fn llm_splits_only_when_context_exceeds_limit_and_preserves_text() {
    let text = "第一段。\n第二段。\n第三段。";
    let parts = easytranslate::article::split_llm(text, 5);
    let rebuilt = parts
        .iter()
        .map(|part| match part {
            easytranslate::article::Part::Translate(value)
            | easytranslate::article::Part::Whitespace(value) => value.as_str(),
        })
        .collect::<String>();
    assert_eq!(rebuilt, text);
    assert!(parts.len() > 1);
}

#[test]
fn online_prefers_complete_sentences_to_a_later_word_boundary() {
    let first = format!("{}.", "a".repeat(299));
    let next = format!("{}?", "word ".repeat(60).trim_end());
    let parts = easytranslate::article::split_online(&format!("{first} {next}"));
    assert_eq!(
        parts,
        vec![
            Part::Translate(first),
            Part::Whitespace(" ".into()),
            Part::Translate(next)
        ]
    );
}
