#![cfg(target_os = "macos")]

#[test]
fn siliconflow_diagnostics_report_key_presence_without_revealing_it_or_testing_the_network() {
    let home = std::env::temp_dir().join(format!(
        "easytranslate-local-diagnostics-{}",
        std::process::id()
    ));
    let config_dir = home.join("Library/Application Support/EasyTranslate");
    std::fs::create_dir_all(&config_dir).unwrap();
    let mut config = easytranslate::shortcuts::Config {
        provider: easytranslate::translation::Provider::SiliconFlow,
        siliconflow_api_key: "private-diagnostic-key".into(),
        ..Default::default()
    };
    config
        .services
        .push(easytranslate::service::ServiceConfig::compatible(
            "compat-test",
            "硅基流动",
            "https://api.siliconflow.cn/v1/chat/completions",
            "Qwen/Qwen3-32B",
            "private-diagnostic-key",
        ));
    config.active_service = config.services.len() - 1;
    config.save_to(&config_dir.join("config.json")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_easytranslate"))
        .arg("--diagnostics")
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("硅基流动"), "{stdout}");
    assert!(stdout.contains("硅基流动 API Key：已配置"), "{stdout}");
    assert!(stdout.contains("联网翻译：未启用"), "{stdout}");
    assert!(stdout.contains("不会发送文字或测试网络"), "{stdout}");
    assert!(!stdout.contains("private-diagnostic-key"), "{stdout}");
    assert!(!stdout.contains("Chinese–English model:"), "{stdout}");
    assert!(
        !home
            .join("Library/Application Support/EasyTranslate/Models")
            .exists()
    );
    std::fs::remove_dir_all(home).unwrap();
}
