use easytranslate::local_ai::{
    GlossaryEntry, Tone, apply_glossary_to_result, apply_glossary_to_source, build_messages,
    seeded_glossary,
};

#[test]
fn customer_service_is_the_default_tone_and_serializes_stably() {
    let tone = Tone::default();
    assert_eq!(tone, Tone::CustomerService);
    assert_eq!(
        serde_json::to_string(&tone).unwrap(),
        "\"customer_service\""
    );
    assert_eq!(
        serde_json::from_str::<Tone>("\"faithful\"").unwrap(),
        Tone::Faithful
    );
}

#[test]
fn seeded_it_glossary_contains_bidirectional_support_terms() {
    let glossary = seeded_glossary();
    assert!(glossary.iter().any(|entry| {
        entry.source == "固件"
            && entry.target == "firmware"
            && entry.source_language == "zh-Hans"
            && entry.target_language == "en"
    }));
    assert!(glossary.iter().any(|entry| {
        entry.source == "firmware"
            && entry.target == "固件"
            && entry.source_language == "en"
            && entry.target_language == "zh-Hans"
    }));
}

#[test]
fn prompts_include_only_enabled_matching_terms_present_in_source_and_keep_longest_first() {
    let mut glossary = seeded_glossary();
    glossary.push(GlossaryEntry::new("驱动", "driver", "zh-Hans", "en"));
    glossary.push(GlossaryEntry::new(
        "驱动程序",
        "device driver",
        "zh-Hans",
        "en",
    ));
    glossary.push(GlossaryEntry::new("固件", "firmware", "zh-Hans", "en"));
    glossary.push(GlossaryEntry {
        source: "程序".into(),
        target: "program".into(),
        source_language: "zh-Hans".into(),
        target_language: "en".into(),
        note: String::new(),
        enabled: false,
    });
    let messages = build_messages(
        "请更新驱动程序。",
        "zh-Hans",
        "en",
        Tone::CustomerService,
        &glossary,
    );
    assert_eq!(messages.len(), 2);
    assert!(messages[0].content.contains("device driver"));
    assert!(!messages[0].content.contains("driver → driver"));
    assert!(!messages[0].content.contains("firmware"));
    assert!(!messages[0].content.contains("program"));
    assert!(messages[1].content.contains("请更新驱动程序。"));
}

#[test]
fn every_tone_preserves_facts_and_has_a_distinct_style_instruction() {
    let tones = [
        Tone::CustomerService,
        Tone::BusinessFormal,
        Tone::Friendly,
        Tone::Concise,
        Tone::Faithful,
    ];
    let mut instructions: Vec<_> = tones
        .into_iter()
        .map(|tone| {
            build_messages("请勿重置设备。", "zh-Hans", "en", tone, &[])[0]
                .content
                .clone()
        })
        .collect();
    for instruction in &instructions {
        assert!(instruction.contains("否定"));
        assert!(instruction.contains("金额"));
        assert!(instruction.contains("不得补充"));
    }
    instructions.sort();
    instructions.dedup();
    assert_eq!(instructions.len(), 5);
}

#[test]
fn glossary_terms_are_injected_without_rewriting_unrelated_text() {
    let glossary = seeded_glossary();
    assert_eq!(
        apply_glossary_to_source("请更新固件并检查网络适配器。", "zh-Hans", "en", &glossary,),
        "请更新firmware并检查network adapter。"
    );
    assert_eq!(
        apply_glossary_to_result(
            "Please update the 固件 and the network adapter.",
            "zh-Hans",
            "en",
            &glossary,
        ),
        "Please update the firmware and the network adapter."
    );
}

#[test]
fn english_glossary_matching_is_case_insensitive() {
    let glossary = seeded_glossary();
    assert_eq!(
        apply_glossary_to_source("Please update the FIRMWARE.", "en", "zh-Hans", &glossary),
        "Please update the 固件."
    );
    assert_eq!(
        apply_glossary_to_source(
            "用户说：FIRMWARE needs an update.",
            "en",
            "zh-Hans",
            &glossary
        ),
        "用户说：固件 needs an update."
    );
}
