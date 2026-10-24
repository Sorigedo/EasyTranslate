use easytranslate::{
    service::{ServiceConfig, ServiceKind},
    shortcuts::Config,
};

#[test]
fn default_config_has_local_google_and_one_openai_cli_service() {
    let config = Config::default();
    assert_eq!(config.services.len(), 4);
    assert_eq!(config.services[0].kind, ServiceKind::LocalAi);
    assert_eq!(config.services[0].name, "本地开源模型");
    assert_eq!(config.services[1].kind, ServiceKind::Google);
    assert_eq!(config.services[2].kind, ServiceKind::LibreTranslate);
    assert_eq!(config.services[3].kind, ServiceKind::OpenAiCli);
    assert!(config.services[3].model.is_empty());
    assert_eq!(config.active_service, 0);
}

#[test]
fn first_comparison_defaults_use_google_local_and_an_installed_12b_ollama_model() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(
        &mut services,
        &["qwen3:8b".into(), "qwen3:12b".into()],
    );

    let selected = easytranslate::service::default_comparison_services(&services);

    assert_eq!(selected, ["google", "local-1", "ollama:qwen3:12b"]);
}

#[test]
fn removed_and_duplicate_comparison_services_are_replaced_with_available_defaults() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(&mut services, &["qwen3:12b".into()]);
    let mut selected = vec!["removed-service".into(), "google".into(), "google".into()];

    easytranslate::service::normalize_comparison_services(&services, &mut selected);

    assert_eq!(selected, ["google", "local-1", "ollama:qwen3:12b"]);
}

#[test]
fn legacy_configs_migrate_to_the_service_list() {
    let siliconflow = Config::from_json(
        r#"{"provider":"silicon_flow","online_enabled":true,"siliconflow_endpoint":"https://example.test/v1","siliconflow_model":"Qwen/Qwen3-32B","siliconflow_api_key":"k"}"#,
    )
    .unwrap();
    assert_eq!(siliconflow.services.len(), 5);
    assert_eq!(siliconflow.services[4].kind, ServiceKind::OpenAiCompatible);
    assert_eq!(siliconflow.services[4].endpoint, "https://example.test/v1");
    assert_eq!(siliconflow.services[4].api_key, "k");
    assert_eq!(siliconflow.active_service, 4);
    assert!(siliconflow.online_enabled);

    let llm = Config::from_json(r#"{"provider":"llm","llm_model":"gpt-6-sol"}"#).unwrap();
    assert_eq!(llm.services[3].model, "gpt-6-sol");
    assert_eq!(llm.active_service, 3);

    let deepseek = Config::from_json(r#"{"provider":"deep_seek"}"#).unwrap();
    assert_eq!(deepseek.services[4].kind, ServiceKind::OpenAiCompatible);
    assert_eq!(deepseek.active_service, 4);

    let local = Config::from_json(r#"{"provider":"local_ai"}"#).unwrap();
    assert_eq!(local.active_service, 0);

    let google = Config::from_json(r#"{"provider":"google","online_enabled":true}"#).unwrap();
    assert_eq!(
        google.services[google.active_service].kind,
        ServiceKind::Google
    );
    assert!(google.online_enabled);
}

#[test]
fn services_roundtrip_through_json_and_new_entries_persist() {
    let mut config = Config::default();
    config.services.push(ServiceConfig {
        id: "compat-1".into(),
        name: "DeepSeek".into(),
        kind: ServiceKind::OpenAiCompatible,
        endpoint: "https://api.deepseek.com/v1".into(),
        model: "deepseek-chat".into(),
        api_key: "sk-test".into(),
    });
    config.active_service = 4;
    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(restored.services.len(), 5);
    assert_eq!(restored.services[4].name, "DeepSeek");
    assert_eq!(restored.services[4].api_key, "sk-test");
    assert_eq!(restored.active_service, 4);
}

#[test]
fn libretranslate_endpoint_and_api_key_persist_on_its_service_entry() {
    let mut config = Config::default();
    let service = config
        .services
        .iter_mut()
        .find(|service| service.kind == ServiceKind::LibreTranslate)
        .unwrap();
    service.endpoint = "http://127.0.0.1:5000".into();
    service.api_key = "private-key".into();

    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();
    let service = restored
        .services
        .iter()
        .find(|service| service.kind == ServiceKind::LibreTranslate)
        .unwrap();
    assert_eq!(service.endpoint, "http://127.0.0.1:5000");
    assert_eq!(service.api_key, "private-key");
}

#[test]
fn older_saved_service_lists_gain_libretranslate_without_losing_active_service() {
    let json = serde_json::json!({
        "provider": "llm",
        "active_service": 2,
        "services": [
            ServiceConfig::local("local-1", "本地开源模型"),
            ServiceConfig { id: "google".into(), name: "谷歌翻译".into(), kind: ServiceKind::Google, endpoint: String::new(), model: String::new(), api_key: String::new() },
            ServiceConfig::openai_cli("openai-cli-1", "OpenAI（本地 CLI）", "")
        ]
    });
    let restored = Config::from_json(&json.to_string()).unwrap();

    assert_eq!(restored.services[2].kind, ServiceKind::LibreTranslate);
    assert_eq!(restored.services[3].kind, ServiceKind::OpenAiCli);
    assert_eq!(restored.active_service, 3);
    assert_eq!(restored.provider, easytranslate::translation::Provider::Llm);
}

#[test]
fn active_service_selection_is_validated_on_load() {
    let config = Config {
        active_service: 99,
        ..Default::default()
    };
    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(restored.active_service, 0);
}

#[test]
fn adding_a_service_extends_the_picker_entries() {
    let mut config = Config::default();
    let before = config.services.len();
    config.services.push(ServiceConfig {
        id: "local-2".into(),
        name: "本地-模型2".into(),
        kind: ServiceKind::LocalAi,
        endpoint: String::new(),
        model: String::new(),
        api_key: String::new(),
    });
    assert_eq!(config.services.len(), before + 1);
    assert_eq!(config.services.last().unwrap().name, "本地-模型2");
}

#[test]
fn syncing_ollama_models_adds_each_model_as_a_selectable_service() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(
        &mut services,
        &["qwen3:8b".into(), "llama3.2:latest".into()],
    );

    let local_models = services
        .iter()
        .filter(|service| service.kind == ServiceKind::Ollama)
        .collect::<Vec<_>>();
    assert_eq!(local_models.len(), 2);
    assert_eq!(local_models[0].id, "ollama:qwen3:8b");
    assert_eq!(local_models[0].name, "Ollama · qwen3:8b");
    assert_eq!(local_models[0].model, "qwen3:8b");
    assert_eq!(local_models[1].model, "llama3.2:latest");
    assert_eq!(services[1].kind, ServiceKind::Ollama);
    assert_eq!(services[2].kind, ServiceKind::Ollama);
    assert_eq!(services[3].kind, ServiceKind::Google);
    assert_eq!(services[4].kind, ServiceKind::LibreTranslate);
}

#[test]
fn syncing_ollama_models_removes_uninstalled_entries_without_duplicates() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(
        &mut services,
        &["qwen3:8b".into(), "llama3.2".into()],
    );
    easytranslate::service::sync_ollama_models(&mut services, &["llama3.2".into()]);
    easytranslate::service::sync_ollama_models(&mut services, &["llama3.2".into()]);

    let local_models = services
        .iter()
        .filter(|service| service.kind == ServiceKind::Ollama)
        .collect::<Vec<_>>();
    assert_eq!(local_models.len(), 1);
    assert_eq!(local_models[0].model, "llama3.2");
}

#[test]
fn comparison_service_picker_lists_every_service_for_direct_swapping() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(
        &mut services,
        &["qwen3:8b".into(), "qwen3:12b".into()],
    );
    let choices = easytranslate::service::comparison_service_choices(&services);

    assert_eq!(
        choices,
        [
            "local-1",
            "ollama:qwen3:8b",
            "ollama:qwen3:12b",
            "google",
            "libretranslate",
            "openai-cli-1",
        ]
    );
}

#[test]
fn choosing_a_service_used_by_another_card_swaps_the_two_card_assignments() {
    let mut services = Config::default().services;
    easytranslate::service::sync_ollama_models(&mut services, &["qwen3:12b".into()]);
    let selected = vec![
        "google".to_owned(),
        "local-1".to_owned(),
        "ollama:qwen3:12b".to_owned(),
    ];

    let updated =
        easytranslate::service::assign_comparison_service(&services, &selected, 0, "local-1")
            .unwrap();

    assert_eq!(updated, ["local-1", "google", "ollama:qwen3:12b"]);
}

#[test]
fn comparison_service_choices_survive_config_roundtrip() {
    let mut config = Config::default();
    easytranslate::service::sync_ollama_models(
        &mut config.services,
        &["qwen3:8b".into(), "qwen3:12b".into()],
    );
    config.comparison_services = vec![
        "ollama:qwen3:8b".into(),
        "google".into(),
        "ollama:qwen3:12b".into(),
    ];

    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();

    assert_eq!(
        restored.comparison_services,
        ["ollama:qwen3:8b", "google", "ollama:qwen3:12b"]
    );
}

#[test]
fn each_service_kind_maps_to_its_translation_backend() {
    use easytranslate::translation::Provider;

    let local_model = ServiceConfig::ollama("qwen3:8b");
    assert_eq!(local_model.provider(), Provider::LocalModel);
    assert_eq!(
        ServiceConfig::local("marian", "Marian").provider(),
        Provider::LocalAi
    );
    assert_eq!(Config::default().services[1].provider(), Provider::Google);
    assert_eq!(
        Config::default().services[2].provider(),
        Provider::LibreTranslate
    );
    assert_eq!(Config::default().services[3].provider(), Provider::Llm);
}

#[test]
fn selected_ollama_service_uses_its_own_model_instead_of_the_codex_model() {
    let ollama = ServiceConfig::ollama("qwen2.5:3b-instruct");

    assert_eq!(
        ollama.model_for_translation("gpt-6.1-sol"),
        "qwen2.5:3b-instruct"
    );
    assert_eq!(
        ServiceConfig::openai_cli("codex", "OpenAI", "").model_for_translation("gpt-6.1-sol"),
        "gpt-6.1-sol"
    );
}

#[test]
fn active_service_kind_wins_over_a_stale_provider_field() {
    let mut config = Config::default();
    config
        .services
        .push(ServiceConfig::ollama("qwen2.5:3b-instruct"));
    config.active_service = config.services.len() - 1;
    config.provider = easytranslate::translation::Provider::Llm;

    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();

    assert_eq!(
        restored.provider,
        easytranslate::translation::Provider::LocalModel
    );
    assert_eq!(
        restored.services[restored.active_service].model,
        "qwen2.5:3b-instruct"
    );
}
