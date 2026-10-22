use easytranslate::{
    cache::Cache,
    local_ai::{LocalTranslator, RouteLoader, TranslationRoute},
    translation::{self, Options, Provider},
};
use std::sync::{Arc, Mutex, atomic::AtomicU64};

#[test]
fn llm_provider_uses_openai_label_in_the_translation_ui() {
    assert_eq!(Provider::Llm.label(), "OpenAI / 本机 Codex");
}

#[test]
fn bundled_services_default_to_local_and_siliconflow_settings_roundtrip() {
    let defaults = easytranslate::shortcuts::Config::default();
    assert_eq!(defaults.provider, Provider::LocalAi);
    assert_eq!(defaults.active_service, 0);
    assert_eq!(
        defaults.siliconflow_endpoint,
        "https://api.siliconflow.cn/v1/chat/completions"
    );
    assert!(defaults.siliconflow_api_key.is_empty());

    let config = easytranslate::shortcuts::Config::from_json(
        r#"{"provider":"silicon_flow","siliconflow_endpoint":"https://example.test/v1","siliconflow_model":"Qwen/Qwen3-32B","siliconflow_api_key":"private-test-key"}"#,
    )
    .unwrap();
    assert_eq!(config.provider, Provider::SiliconFlow);
    assert_eq!(config.siliconflow_endpoint, "https://example.test/v1");
    assert_eq!(config.siliconflow_model, "Qwen/Qwen3-32B");
    assert_eq!(config.siliconflow_api_key, "private-test-key");
    let restored =
        easytranslate::shortcuts::Config::from_json(&serde_json::to_string(&config).unwrap())
            .unwrap();
    assert_eq!(restored.siliconflow_api_key, "private-test-key");
}

#[test]
fn deepseek_migrates_to_siliconflow_while_openai_codex_remains_selectable() {
    for previous in ["deep_seek", "deep_seek_pro"] {
        let config = easytranslate::shortcuts::Config::from_json(&format!(
            r#"{{"provider":"{previous}","online_enabled":true}}"#
        ))
        .unwrap();
        assert_eq!(config.provider, Provider::SiliconFlow);
        assert!(config.online_enabled);
    }

    let config =
        easytranslate::shortcuts::Config::from_json(r#"{"provider":"llm","online_enabled":true}"#)
            .unwrap();
    assert_eq!(config.provider, Provider::Llm);
    assert!(config.online_enabled);
}

#[test]
fn provider_configuration_keeps_openai_codex_as_a_third_service() {
    let config = easytranslate::shortcuts::Config::from_json(
        r#"{"provider":"llm","online_enabled":true,"llm_model":"gpt-6-sol"}"#,
    )
    .unwrap();
    assert_eq!(config.provider, Provider::Llm);
    assert_eq!(Provider::Llm.label(), "OpenAI / 本机 Codex");
}

#[test]
fn siliconflow_provider_requires_online_consent() {
    let options = Options {
        provider: Provider::SiliconFlow,
        online_enabled: false,
        siliconflow_endpoint: String::new(),
        siliconflow_model: String::new(),
        siliconflow_api_key: String::new(),
        codex_model: String::new(),
        local_translator: None,
        tone: easytranslate::local_ai::Tone::Faithful,
        glossary: Vec::new(),
    };
    assert!(options.check().unwrap_err().contains("启用联网翻译"));
}

#[test]
fn saved_siliconflow_settings_are_used_by_the_translation_request() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 2048];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let header = String::from_utf8_lossy(&request[..header_end]);
        let content_length = header
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let count = stream.read(&mut chunk).unwrap();
            request.extend_from_slice(&chunk[..count]);
        }
        let response = r#"{"choices":[{"message":{"content":"Please confirm by Friday."}}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .unwrap();
        String::from_utf8(request).unwrap()
    });

    let config_path = std::env::temp_dir().join(format!(
        "easytranslate-siliconflow-route-{}-{}.json",
        std::process::id(),
        address.port()
    ));
    let mut saved = easytranslate::shortcuts::Config {
        provider: Provider::SiliconFlow,
        online_enabled: true,
        siliconflow_endpoint: format!("http://{address}/v1"),
        siliconflow_model: "Qwen/Qwen3-32B-test".into(),
        siliconflow_api_key: "saved-key-test".into(),
        ..Default::default()
    };
    saved
        .services
        .push(easytranslate::service::ServiceConfig::compatible(
            "compat-test",
            "硅基流动",
            saved.siliconflow_endpoint.as_str(),
            saved.siliconflow_model.as_str(),
            saved.siliconflow_api_key.as_str(),
        ));
    saved.active_service = saved.services.len() - 1;
    saved.save_to(&config_path).unwrap();
    let loaded = easytranslate::shortcuts::Config::load_from(&config_path).unwrap();

    let result = translation::run(
        Options {
            provider: loaded.provider,
            online_enabled: loaded.online_enabled,
            siliconflow_endpoint: loaded.siliconflow_endpoint,
            siliconflow_model: loaded.siliconflow_model,
            siliconflow_api_key: loaded.siliconflow_api_key,
            codex_model: loaded.llm_model,
            local_translator: None,
            tone: easytranslate::local_ai::Tone::Faithful,
            glossary: Vec::new(),
        },
        "请在周五前确认。",
        "zh-Hans",
        "en",
        (Arc::new(AtomicU64::new(1)), 1),
        Arc::new(Mutex::new(Cache::new(8))),
        |_, _| {},
    )
    .unwrap();
    let request = server.join().unwrap();
    std::fs::remove_file(config_path).unwrap();

    let header = request.split("\r\n\r\n").next().unwrap();
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(header.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(header.contains("Authorization: Bearer saved-key-test"));
    assert_eq!(body["model"], "Qwen/Qwen3-32B-test");
    assert_eq!(result.text, "Please confirm by Friday.");
    assert!(!result.used_fallback);
}

struct StubRoute;
impl TranslationRoute for StubRoute {
    fn translate_long(&self, text: &str) -> String {
        format!("译:{text}")
    }
}

struct StubLoader;
impl RouteLoader for StubLoader {
    fn load(&self, _source: &str, _target: &str) -> Result<Box<dyn TranslationRoute>, String> {
        Ok(Box::new(StubRoute))
    }
}

#[test]
fn local_provider_translates_each_paragraph_and_preserves_blank_lines() {
    let local = Arc::new(LocalTranslator::with_loader(Arc::new(StubLoader)));
    let result = translation::run(
        Options {
            provider: Provider::LocalAi,
            online_enabled: false,
            siliconflow_endpoint: String::new(),
            siliconflow_model: String::new(),
            siliconflow_api_key: String::new(),
            codex_model: String::new(),
            local_translator: Some(local),
            tone: easytranslate::local_ai::Tone::Faithful,
            glossary: Vec::new(),
        },
        "你好。\n\n再见。",
        "zh-Hans",
        "en",
        (Arc::new(AtomicU64::new(1)), 1),
        Arc::new(Mutex::new(Cache::new(32))),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(result.text, "译:你好。\n\n译:再见。");
}

#[test]
fn local_provider_cache_key_changes_when_tone_or_glossary_changes() {
    use easytranslate::cache::Key;
    let cache = Arc::new(Mutex::new(Cache::new(8)));
    let local = Arc::new(LocalTranslator::with_loader(Arc::new(StubLoader)));
    let base_options = Options {
        provider: Provider::LocalAi,
        online_enabled: false,
        siliconflow_endpoint: String::new(),
        siliconflow_model: String::new(),
        siliconflow_api_key: String::new(),
        codex_model: String::new(),
        local_translator: Some(local),
        tone: easytranslate::local_ai::Tone::CustomerService,
        glossary: easytranslate::local_ai::seeded_glossary(),
    };
    let text = "请更新固件。";
    translation::run(
        base_options.clone(),
        text,
        "zh-Hans",
        "en",
        (Arc::new(AtomicU64::new(1)), 1),
        cache.clone(),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(cache.lock().unwrap().len(), 1);
    let mut changed = base_options;
    changed.tone = easytranslate::local_ai::Tone::Concise;
    translation::run(
        changed,
        text,
        "zh-Hans",
        "en",
        (Arc::new(AtomicU64::new(1)), 1),
        cache.clone(),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(cache.lock().unwrap().len(), 2);
    let customer_key = Key {
        source: "local_ai:zh-Hans".into(),
        target: "en".into(),
        text: text.into(),
        profile: easytranslate::local_ai::profile_key(
            easytranslate::local_ai::Tone::CustomerService,
            &easytranslate::local_ai::seeded_glossary(),
        ),
    };
    let concise_key = Key {
        profile: easytranslate::local_ai::profile_key(
            easytranslate::local_ai::Tone::Concise,
            &easytranslate::local_ai::seeded_glossary(),
        ),
        ..customer_key.clone()
    };
    let cache = cache.lock().unwrap();
    assert!(cache.contains(&customer_key));
    assert!(cache.contains(&concise_key));
}
