use easytranslate::shortcuts::{Config, Shortcut, validate};
#[test]
fn parses_user_shortcuts_and_rejects_unmodified_typing() {
    let a = Shortcut::parse("Option+A").unwrap();
    assert_eq!((a.code, a.modifiers), (0, 2048));
    assert_eq!(Shortcut::parse("⌥A").unwrap(), a);
    assert_eq!(Shortcut::parse("Ctrl+Shift+F8").unwrap().modifiers, 4608);
    assert!(Shortcut::parse("A").is_err());
    assert!(Shortcut::parse("Fn+A").is_err());
}

#[test]
fn option_a_toggles_the_panel_and_option_d_translates_selection() {
    let config = Config::default();

    assert_eq!(config.shortcuts[0], Shortcut::parse("Option+A").unwrap());
    assert_eq!(config.shortcuts[1], Shortcut::parse("Option+D").unwrap());
}

#[test]
fn migrates_v2_option_d_toggle_and_option_a_selection_to_the_new_roles() {
    let config = Config::from_json(
        r#"{"shortcut_version":2,"shortcuts":[{"code":2,"modifiers":2048},{"code":0,"modifiers":2048},{"code":1,"modifiers":2304}]}"#,
    )
    .unwrap();

    assert_eq!(config.shortcuts[0], Shortcut::parse("Option+A").unwrap());
    assert_eq!(config.shortcuts[1], Shortcut::parse("Option+D").unwrap());
    assert_eq!(
        config.shortcuts[2],
        Shortcut::parse("Command+Option+S").unwrap()
    );
}

#[test]
fn migration_swaps_old_shortcut_roles_without_losing_the_screenshot_key() {
    let config = Config::from_json(
        r#"{"shortcut_version":2,"shortcuts":[{"code":12,"modifiers":4096},{"code":13,"modifiers":256},{"code":1,"modifiers":2816}]}"#,
    )
    .unwrap();

    assert_eq!(config.shortcuts[0], Shortcut::parse("Command+W").unwrap());
    assert_eq!(config.shortcuts[1], Shortcut::parse("Control+Q").unwrap());
    assert_eq!(
        config.shortcuts[2],
        Shortcut::parse("Command+Option+Shift+S").unwrap()
    );
}
#[test]
fn rejects_duplicates() {
    let mut keys = Config::default().shortcuts;
    keys[1] = keys[0];
    assert!(validate(&keys).is_err());
    keys[1] = Shortcut::parse("Ctrl+A").unwrap();
    assert!(validate(&keys).is_ok());
}
#[test]
fn migrates_old_defaults_and_preserves_custom_keys() {
    let defaults = Config::from_json(r#"{"hotkey_codes":[49,2,1],"target":"ja"}"#).unwrap();
    assert_eq!(defaults.shortcuts[0], Shortcut::parse("Option+A").unwrap());
    assert_eq!(defaults.target, "ja");
    let custom = Config::from_json(r#"{"hotkey_codes":[12,13,14]}"#).unwrap();
    assert_eq!(
        custom.shortcuts[0],
        Shortcut::parse("Command+Option+Q").unwrap()
    );
    assert_eq!(custom.shortcuts[1], Shortcut::parse("Option+D").unwrap());
}
#[test]
fn saved_settings_roundtrip_without_losing_modifiers() {
    let mut config = Config::default();
    config.shortcuts[0] = Shortcut::parse("Control+Shift+F8").unwrap();
    let json = serde_json::to_string(&config).unwrap();
    let restored = Config::from_json(&json).unwrap();
    assert_eq!(restored.shortcuts, config.shortcuts);
    assert_eq!(
        Shortcut::parse(&restored.shortcuts[0].label()).unwrap(),
        restored.shortcuts[0]
    );
}
#[test]
fn invalid_configuration_reports_error_without_overwriting_file() {
    let path = std::env::temp_dir().join(format!(
        "easytranslate-config-test-{}.json",
        std::process::id()
    ));
    let invalid = r#"{"target":"ja","shortcuts":"invalid"}"#;
    std::fs::write(&path, invalid).unwrap();
    assert!(Config::load_from(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    std::fs::remove_file(&path).unwrap();
    assert!(Config::load_from(&path).is_ok());
}
#[test]
fn removed_remote_providers_migrate_to_local_mode() {
    let config = Config::default();
    assert_eq!(
        config.provider,
        easytranslate::translation::Provider::LocalAi
    );
    assert!(!config.online_enabled);
    let migrated = Config::from_json(
        r#"{"provider":"my_memory","online_enabled":true,"target":"ja","hotkey_codes":[12,13,14]}"#,
    )
    .unwrap();
    assert_eq!(
        migrated.provider,
        easytranslate::translation::Provider::LocalAi
    );
    assert_eq!(migrated.target, "ja");
    assert_eq!(
        migrated.shortcuts[0],
        Shortcut::parse("Command+Option+Q").unwrap()
    );
    assert!(!migrated.online_enabled);
    let restored = Config::from_json(r#"{"provider":"apple","online_enabled":false}"#).unwrap();
    assert_eq!(
        restored.provider,
        easytranslate::translation::Provider::LocalAi,
        "removed Apple provider should migrate to the offline local provider"
    );
    let google = Config::from_json(r#"{"provider":"google","online_enabled":true}"#).unwrap();
    assert_eq!(
        google.provider,
        easytranslate::translation::Provider::Google
    );
    assert!(google.online_enabled);
    for provider in ["my_memory", "iflytek", "youdao"] {
        let migrated = Config::from_json(&format!(
            r#"{{"provider":"{provider}","online_enabled":true}}"#
        ))
        .unwrap();
        assert_eq!(
            migrated.provider,
            easytranslate::translation::Provider::LocalAi
        );
        assert!(!migrated.online_enabled);
    }
}

#[test]
fn older_config_without_tone_or_glossary_gets_local_defaults() {
    let config = Config::from_json(
        r#"{"provider":"local_ai","online_enabled":false,"target":"en","reading_target":"zh-Hans","shortcuts":[{"code":0,"modifiers":2048},{"code":2,"modifiers":2048},{"code":1,"modifiers":2304}]}"#,
    )
    .unwrap();
    assert_eq!(config.tone, easytranslate::local_ai::Tone::CustomerService);
    assert!(
        config
            .glossary
            .iter()
            .any(|entry| entry.source == "固件" && entry.target == "firmware")
    );
}

#[test]
fn model_override_is_optional_and_persists_in_settings() {
    let config = Config::from_json(
        r#"{"provider":"local_ai","llm_model":"gpt-6-sol","shortcuts":[{"code":0,"modifiers":2048},{"code":2,"modifiers":2048},{"code":1,"modifiers":2304}]}"#,
    )
    .unwrap();
    assert_eq!(config.llm_model, "gpt-6-sol");
    let restored = Config::from_json(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(restored.llm_model, "gpt-6-sol");
}

#[cfg(unix)]
#[test]
fn siliconflow_api_key_persists_in_an_owner_only_config_file() {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::temp_dir().join(format!(
        "easytranslate-siliconflow-config-test-{}.json",
        std::process::id()
    ));
    let config = Config {
        siliconflow_api_key: "private-test-key".into(),
        ..Default::default()
    };
    config.save_to(&path).unwrap();

    let restored = Config::load_from(&path).unwrap();
    assert_eq!(restored.siliconflow_api_key, "private-test-key");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recorded_native_keys_convert_modifiers_and_ignore_caps_lock() {
    let shortcut = Shortcut::from_recorded_event(0, (1 << 19) | (1 << 16)).unwrap();
    assert_eq!(shortcut, Shortcut::parse("Option+A").unwrap());
    assert_eq!(
        Shortcut::from_recorded_event(100, (1 << 18) | (1 << 17) | (1 << 23)).unwrap(),
        Shortcut::parse("Control+Shift+F8").unwrap()
    );
    assert!(Shortcut::from_recorded_event(0, 0).is_err());
    assert!(Shortcut::from_recorded_event(0, 1 << 17).is_err());
    assert!(Shortcut::from_recorded_event(53, 1 << 20).is_err());
}
