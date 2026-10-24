//! Native backend integration only: never opens windows or sends input events.
use easytranslate::{cache::Cache, local_ai::LocalTranslator, translation};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
fn main() {
    let cache = Arc::new(Mutex::new(Cache::new(32)));
    let cancellation = Arc::new(AtomicU64::new(1));
    let local_options = translation::Options {
        provider: translation::Provider::LocalAi,
        online_enabled: false,
        siliconflow_endpoint: easytranslate::llm::SILICONFLOW_DEFAULT_ENDPOINT.into(),
        siliconflow_model: easytranslate::llm::SILICONFLOW_DEFAULT_MODEL.into(),
        siliconflow_api_key: String::new(),
        libretranslate_endpoint: String::new(),
        libretranslate_api_key: String::new(),
        codex_model: String::new(),
        local_translator: Some(Arc::new(LocalTranslator::new(
            LocalTranslator::default_cache_root(),
        ))),
        tone: easytranslate::local_ai::Tone::Faithful,
        glossary: Vec::new(),
    };
    let mut progress = Vec::new();
    let first = translation::run(
        local_options.clone(),
        "Ra\nPlease confirm invoice number 7351.",
        "auto",
        "zh-Hans",
        (cancellation.clone(), 1),
        cache.clone(),
        |done, total| progress.push((done, total)),
    )
    .unwrap();
    assert!(
        first.text.contains("7351") && first.text.contains("发票"),
        "Whole-document auto detection failed: {}",
        first.text
    );
    assert_eq!(progress.last(), Some(&(2, 2)), "Missing terminal progress");
    let second = translation::run(
        local_options.clone(),
        "请明天下午联系我。",
        "zh-Hans",
        "en",
        (cancellation.clone(), 1),
        cache.clone(),
        |_, _| {},
    )
    .unwrap();
    assert!(
        second.text.to_lowercase().contains("tomorrow"),
        "Wrong replacement source: {}",
        second.text
    );
    cancellation.store(2, Ordering::Relaxed);
    let cancelled = translation::run(
        local_options.clone(),
        "请明天下午联系我。",
        "zh-Hans",
        "en",
        (cancellation.clone(), 1),
        cache.clone(),
        |_, _| {},
    );
    assert!(
        cancelled.is_err(),
        "Cancelled operation must not return cached translation"
    );
    let third = translation::run(
        local_options.clone(),
        "Please confirm invoice number 7351.",
        "en",
        "zh-Hans",
        (cancellation.clone(), 2),
        cache.clone(),
        |_, _| {},
    )
    .unwrap();
    assert!(third.text.contains("发票"));
    let cancelled_between = translation::run(
        local_options.clone(),
        "请明天下午联系我。\n请明天下午联系我。",
        "zh-Hans",
        "en",
        (cancellation.clone(), 2),
        cache,
        |done, _| {
            if done == 1 {
                cancellation.store(3, Ordering::Relaxed)
            }
        },
    );
    assert!(
        cancelled_between.is_err(),
        "Cancellation between cached segments was ignored"
    );
    let generation = cancellation.load(Ordering::Relaxed);
    let long_text = (0..30)
        .map(|i| format!("Please confirm invoice number {}.", 7351 + i))
        .collect::<Vec<_>>()
        .join("\n\n");
    let long_result = translation::run(
        local_options,
        &long_text,
        "auto",
        "zh-Hans",
        (cancellation.clone(), generation),
        Arc::new(Mutex::new(Cache::new(32))),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        long_result.text.matches("\n\n").count(),
        29,
        "Paragraph boundaries lost"
    );
    for number in 7351..7381 {
        assert!(
            long_result.text.contains(&number.to_string()),
            "Lost invoice number {number}"
        );
    }
    println!(
        "PASS: native backend whole-document detection, latest input, cache isolation, completion progress, cancellation before cached result and between segments"
    );
}
