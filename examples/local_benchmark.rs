use easytranslate::local_ai::LocalTranslator;
use serde::Deserialize;
use std::time::Instant;

#[derive(Deserialize)]
struct Case {
    id: String,
    text: String,
    must_preserve: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<Case> = serde_json::from_str(include_str!("../docs/quality/chat-cases.json"))?;
    let cache_root = LocalTranslator::default_cache_root();
    let translator = LocalTranslator::new(cache_root.clone());
    println!("Cache: {}", cache_root.display());
    for case in cases {
        let start = Instant::now();
        let result = translator.translate(&case.text, "zh-Hans", "en")?;
        println!(
            "{}\t{} ms\nsource: {}\nexpected meaning: {}\ntranslation: {}",
            case.id,
            start.elapsed().as_millis(),
            case.text,
            case.must_preserve,
            result
        );
    }
    let start = Instant::now();
    let reverse =
        translator.translate("Please do not pay now until I confirm.", "en", "zh-Hans")?;
    println!(
        "reverse_negation\t{} ms\ntranslation: {}",
        start.elapsed().as_millis(),
        reverse
    );
    let cache = fxtranslate::cache::Cache::with_root(cache_root);
    for pair in cache.list_cached()? {
        println!("MODEL\t{}\t{} bytes", pair.name, pair.bytes);
    }
    Ok(())
}
