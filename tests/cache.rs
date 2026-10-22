use easytranslate::cache::{Cache, Key};
fn key(source: &str, target: &str, text: &str) -> Key {
    Key {
        source: source.into(),
        target: target.into(),
        text: text.into(),
        profile: String::new(),
    }
}

#[test]
fn cache_does_not_reuse_a_translation_across_tone_or_glossary_profiles() {
    let mut cache = Cache::new(8);
    let base = Key {
        source: "local_ai:zh-Hans".into(),
        target: "en".into(),
        text: "请更新固件。".into(),
        profile: "customer_service:glossary-v1".into(),
    };
    cache.put(base.clone(), "Please update the firmware.".into());
    let mut other_tone = base.clone();
    other_tone.profile = "concise:glossary-v1".into();
    let mut other_glossary = base.clone();
    other_glossary.profile = "customer_service:glossary-v2".into();
    assert!(cache.get(&other_tone).is_none());
    assert!(cache.get(&other_glossary).is_none());
    assert_eq!(
        cache.get(&base).as_deref(),
        Some("Please update the firmware.")
    );
}
#[test]
fn exact_cache_hit_does_not_cross_language_pairs() {
    let mut c = Cache::new(2);
    c.put(key("zh-Hans", "en", "你好"), "Hello".into());
    assert_eq!(c.get(&key("zh-Hans", "en", "你好")), Some("Hello".into()));
    assert_eq!(c.get(&key("zh-Hans", "fr", "你好")), None);
    assert_eq!(c.get(&key("ja", "en", "你好")), None);
}
#[test]
fn bounded_cache_evicts_least_recently_used_text() {
    let mut c = Cache::new(2);
    c.put(key("a", "b", "1"), "one".into());
    c.put(key("a", "b", "2"), "two".into());
    assert_eq!(c.get(&key("a", "b", "1")), Some("one".into()));
    c.put(key("a", "b", "3"), "three".into());
    assert_eq!(c.get(&key("a", "b", "2")), None);
    assert_eq!(c.get(&key("a", "b", "1")), Some("one".into()));
}
