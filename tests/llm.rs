use easytranslate::llm::{LlmSettings, WireApi, build_request, parse_response};

#[test]
fn saved_model_override_is_applied_to_discovered_settings() {
    let mut settings = LlmSettings {
        endpoint: "https://tokenadvent.com/v1/chat/completions".into(),
        model: "gpt-6-sol".into(),
        api_key: "test-key".into(),
        wire_api: WireApi::ChatCompletions,
    };
    easytranslate::llm::apply_model_override(&mut settings, "gpt-5.2");
    assert_eq!(settings.model, "gpt-5.2");
}

#[test]
fn request_uses_translation_prompt_and_preserves_context() {
    let settings = LlmSettings {
        endpoint: "https://api.siliconflow.cn/v1/chat/completions".into(),
        model: "Qwen/Qwen3-32B".into(),
        api_key: "test-key".into(),
        wire_api: WireApi::ChatCompletions,
    };
    let request = build_request(&settings, "zh-Hans", "en", "请在周五前确认订单。");
    assert_eq!(request["model"], "Qwen/Qwen3-32B");
    assert_eq!(request["messages"][1]["role"], "user");
    assert_eq!(request["messages"][1]["content"], "请在周五前确认订单。");
    let system = request["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("Return only the translation"));
    assert!(system.contains("Preserve the exact meaning"));
}

#[test]
fn siliconflow_request_uses_the_configured_model_and_standard_chat_format() {
    let settings = easytranslate::llm::siliconflow_settings(
        "test-key",
        "https://api.siliconflow.cn/v1",
        "Qwen/Qwen3-32B",
    )
    .unwrap();
    let request = build_request(&settings, "zh-Hans", "en", "请在周五前确认订单。");

    assert_eq!(
        settings.endpoint,
        "https://api.siliconflow.cn/v1/chat/completions"
    );
    assert_eq!(request["model"], "Qwen/Qwen3-32B");
    assert!(request.get("thinking").is_none());
}

#[test]
fn responses_request_and_parser_keep_the_translation_context() {
    let settings = LlmSettings {
        endpoint: "https://tokenadvent.com/v1/responses".into(),
        model: "gpt-6-sol".into(),
        api_key: "test-key".into(),
        wire_api: WireApi::Responses,
    };
    let request = build_request(&settings, "zh-Hans", "en", "请保留订单号 AB-2026。");
    assert_eq!(request["model"], "gpt-6-sol");
    assert!(request.get("input").is_some());
    assert!(request.get("messages").is_none());
    let valid = r#"{"output_text":"Please preserve order number AB-2026."}"#;
    assert_eq!(
        parse_response(valid).unwrap(),
        "Please preserve order number AB-2026."
    );
}

#[test]
fn codex_config_is_parsed_without_printing_or_requiring_the_secret() {
    let auth = r#"{"OPENAI_API_KEY":"secret-value"}"#;
    let config = r#"
model_provider = "MOpenAI"
model = "gpt-6-sol"
[model_providers.MOpenAI]
base_url = "https://tokenadvent.com"
wire_api = "responses"
requires_openai_auth = true
"#;
    let settings = easytranslate::llm::from_codex_files(auth, config).unwrap();
    assert_eq!(settings.endpoint, "https://tokenadvent.com/responses");
    assert_eq!(settings.model, "gpt-6-sol");
    assert_eq!(settings.api_key, "secret-value");
    assert_eq!(settings.wire_api, WireApi::Responses);
}

#[test]
fn response_parser_rejects_empty_or_malformed_model_responses() {
    let valid = r#"{"choices":[{"message":{"content":"Please confirm the order by Friday."}}]}"#;
    assert_eq!(
        parse_response(valid).unwrap(),
        "Please confirm the order by Friday."
    );
    assert!(parse_response(r#"{"choices":[]}"#).is_err());
    assert!(parse_response(r#"{"choices":[{"message":{"content":"  "}}]}"#).is_err());
}

#[test]
fn response_parser_surfaces_provider_error_without_echoing_secrets() {
    let error = r#"{"error":{"message":"invalid api key","type":"authentication_error"}}"#;
    let message = parse_response(error).unwrap_err();
    assert!(message.contains("invalid api key"));
    assert!(!message.contains("test-key"));
}

#[test]
fn siliconflow_settings_use_defaults_and_reject_missing_credentials() {
    let settings =
        easytranslate::llm::siliconflow_settings("siliconflow-test-key", "", "").unwrap();

    assert_eq!(
        settings.endpoint,
        "https://api.siliconflow.cn/v1/chat/completions"
    );
    assert_eq!(settings.model, "Qwen/Qwen3-32B");
    assert_eq!(settings.wire_api, WireApi::ChatCompletions);
    assert!(easytranslate::llm::siliconflow_settings("", "", "").is_err());
}

#[test]
fn openai_fallback_is_not_resolved_when_siliconflow_succeeds() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (primary, server) = local_chat_server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"Please confirm by Friday."}}]}"#,
    );
    let fallback_attempts = AtomicUsize::new(0);
    let result = easytranslate::llm::translate_with_fallback(
        "硅基流动",
        Ok(primary),
        || {
            fallback_attempts.fetch_add(1, Ordering::SeqCst);
            Err("fallback should not be resolved".to_string())
        },
        "请在周五前确认。",
        "zh-Hans",
        "en",
        easytranslate::local_ai::Tone::Faithful,
        &[],
    )
    .unwrap();

    server.join().unwrap();
    assert_eq!(result.text, "Please confirm by Friday.");
    assert!(!result.used_fallback);
    assert_eq!(fallback_attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn openai_is_used_only_after_siliconflow_returns_an_error() {
    let (primary, primary_server) = local_chat_server(
        "503 Service Unavailable",
        r#"{"error":{"message":"temporarily unavailable"}}"#,
    );
    let (fallback, fallback_server) = local_chat_server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"Please confirm by Friday."}}]}"#,
    );
    let result = easytranslate::llm::translate_with_fallback(
        "硅基流动",
        Ok(primary),
        || Ok(fallback),
        "请在周五前确认。",
        "zh-Hans",
        "en",
        easytranslate::local_ai::Tone::Faithful,
        &[],
    )
    .unwrap();

    primary_server.join().unwrap();
    fallback_server.join().unwrap();
    assert_eq!(result.text, "Please confirm by Friday.");
    assert!(result.used_fallback);
}

#[test]
fn direct_codex_translation_labels_runtime_service_errors_as_unavailable() {
    let (settings, server) = local_chat_server(
        "503 Service Unavailable",
        r#"{"error":{"message":"temporarily unavailable"}}"#,
    );
    let error = easytranslate::llm::translate_codex_with_profile(
        &settings,
        "请确认。",
        "zh-Hans",
        "en",
        easytranslate::local_ai::Tone::Faithful,
        &[],
    )
    .unwrap_err();
    server.join().unwrap();

    assert!(error.contains("本机 Codex 不可用"), "{error}");
    assert!(error.contains("temporarily unavailable"), "{error}");
}

#[test]
fn failed_siliconflow_and_unavailable_openai_report_both_causes() {
    let (primary, server) = local_chat_server(
        "503 Service Unavailable",
        r#"{"error":{"message":"temporarily unavailable"}}"#,
    );
    let error = easytranslate::llm::translate_with_fallback(
        "硅基流动",
        Ok(primary),
        || Err("Codex credentials are missing".to_string()),
        "请确认。",
        "zh-Hans",
        "en",
        easytranslate::local_ai::Tone::Faithful,
        &[],
    )
    .unwrap_err();

    server.join().unwrap();
    assert!(error.contains("硅基流动"));
    assert!(error.contains("temporarily unavailable"), "{error}");
    assert!(error.contains("Codex credentials are missing"));
}

#[test]
fn missing_siliconflow_credentials_use_codex_fallback_without_sending_to_siliconflow() {
    let (fallback, fallback_server) = local_chat_server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"Please confirm."}}]}"#,
    );
    let result = easytranslate::llm::translate_with_fallback(
        "硅基流动",
        Err("SiliconFlow API Key is missing".to_string()),
        || Ok(fallback),
        "请确认。",
        "zh-Hans",
        "en",
        easytranslate::local_ai::Tone::Faithful,
        &[],
    )
    .unwrap();

    fallback_server.join().unwrap();
    assert_eq!(result.text, "Please confirm.");
    assert!(result.used_fallback);
}

fn local_chat_server(status: &str, body: &str) -> (LlmSettings, std::thread::JoinHandle<()>) {
    use std::{io::Read, io::Write, net::TcpListener, thread};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_string();
    let body = body.to_string();
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
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (
        LlmSettings {
            endpoint: format!("http://{address}/chat/completions"),
            model: "test-model".into(),
            api_key: "test-key".into(),
            wire_api: WireApi::ChatCompletions,
        },
        server,
    )
}
