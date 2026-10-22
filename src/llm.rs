use crate::local_ai::{ChatMessage, GlossaryEntry, Tone, build_messages};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const GOOGLE_DEFAULT_ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";
pub const OLLAMA_DEFAULT_HOST: &str = "http://127.0.0.1:11434";
const LLM_OVERALL_TIMEOUT: Duration = Duration::from_secs(180);
const OLLAMA_PULL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const OLLAMA_PULL_READ_TIMEOUT: Duration = Duration::from_secs(2);
const OLLAMA_PULL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const OLLAMA_MODEL_KEEP_ALIVE: &str = "5m";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OllamaPullProgress {
    pub status: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}

pub fn google_endpoint() -> String {
    std::env::var("EASYTRANSLATE_GOOGLE_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| GOOGLE_DEFAULT_ENDPOINT.into())
}

pub fn ollama_host() -> String {
    let host = std::env::var("OLLAMA_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| OLLAMA_DEFAULT_HOST.into());
    normalize_ollama_host(&host)
}

fn normalize_ollama_host(value: &str) -> String {
    let host = value.trim().trim_end_matches('/');
    let host = host.strip_suffix("/v1").unwrap_or(host);
    if host.contains("://") {
        host.to_string()
    } else {
        format!("http://{host}")
    }
}

fn normalize_ollama_model_reference(value: &str) -> String {
    let value = value.trim().trim_end_matches('/');
    for prefix in [
        "https://ollama.com/library/",
        "http://ollama.com/library/",
        "ollama.com/library/",
    ] {
        if let Some(model) = value.strip_prefix(prefix) {
            return model.trim_matches('/').to_string();
        }
    }
    if let Some(model) = value.strip_prefix("https://huggingface.co/") {
        return format!("hf.co/{}", model.trim_matches('/'));
    }
    if let Some(model) = value.strip_prefix("http://huggingface.co/") {
        return format!("hf.co/{}", model.trim_matches('/'));
    }
    value.to_string()
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout(LLM_OVERALL_TIMEOUT)
        .build()
}

fn ollama_pull_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(OLLAMA_PULL_READ_TIMEOUT)
        .timeout_write(OLLAMA_PULL_READ_TIMEOUT)
        .build()
}

fn proxy_env_configured() -> bool {
    [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .iter()
    .any(|name| env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

/// Parses the key/value dictionary printed by scutil --proxy.
fn parse_system_proxy_output(output: &str) -> Option<String> {
    let value = |key: &str| {
        output.lines().find_map(|line| {
            let (line_key, line_value) = line.split_once(':')?;
            (line_key.trim() == key).then(|| line_value.trim().to_string())
        })
    };
    let enabled =
        |key: &str| value(key).is_some_and(|item| item == "1" || item.eq_ignore_ascii_case("true"));
    let proxy = |prefix: &str| {
        if !enabled(&format!("{prefix}Enable")) {
            return None;
        }
        let host = value(&format!("{prefix}Proxy")).filter(|host| !host.is_empty())?;
        let port = value(&format!("{prefix}Port"))
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port > 0)?;
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host
        };
        Some(format!("http://{host}:{port}"))
    };

    proxy("HTTPS").or_else(|| proxy("HTTP"))
}

#[cfg(target_os = "macos")]
fn system_proxy_url() -> Option<String> {
    for executable in ["/usr/sbin/scutil", "/usr/bin/scutil"] {
        let output = Command::new(executable)
            .arg("--proxy")
            .output()
            .ok()
            .filter(|output| output.status.success());
        if let Some(output) = output {
            return parse_system_proxy_output(&String::from_utf8_lossy(&output.stdout));
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn system_proxy_url() -> Option<String> {
    None
}

fn online_http_agent() -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout(LLM_OVERALL_TIMEOUT);
    let builder = if proxy_env_configured() {
        builder.try_proxy_from_env(true)
    } else if let Some(proxy_url) = system_proxy_url() {
        match ureq::Proxy::new(proxy_url) {
            Ok(proxy) => builder.proxy(proxy),
            Err(_) => builder.try_proxy_from_env(true),
        }
    } else {
        builder.try_proxy_from_env(true)
    };
    builder.build()
}

fn google_request_error(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Status(status, _) => format!("谷歌翻译服务返回 HTTP {status}"),
        ureq::Error::Transport(transport) => match transport.kind() {
            ureq::ErrorKind::Dns => "无法解析谷歌翻译服务器地址，请检查网络或代理设置".into(),
            ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::ProxyConnect => {
                "无法连接谷歌翻译，请检查网络或代理设置".into()
            }
            ureq::ErrorKind::ProxyUnauthorized => "代理认证失败，请检查代理设置".into(),
            ureq::ErrorKind::InvalidProxyUrl => "代理地址无效，请检查代理设置".into(),
            _ => "谷歌翻译请求失败，请检查网络或代理设置".into(),
        },
    }
}

/// Free Google Translate web endpoint; no API key required.
pub fn google_translate(text: &str, source: &str, target: &str) -> Result<String, String> {
    let sl = if source == "auto" { "auto" } else { source };
    let response = online_http_agent()
        .get(&google_endpoint())
        .query("client", "gtx")
        .query("sl", sl)
        .query("tl", target)
        .query("dt", "t")
        .query("q", text)
        .call()
        .map_err(|error| google_request_error(&error))?;
    let value: serde_json::Value = response
        .into_json()
        .map_err(|error| format!("谷歌翻译返回了无效 JSON：{error}"))?;
    let segments = value
        .get(0)
        .and_then(|entry| entry.as_array())
        .ok_or("谷歌翻译返回格式异常")?;
    let mut result = String::new();
    for segment in segments {
        if let Some(piece) = segment.get(0).and_then(|entry| entry.as_str()) {
            result.push_str(piece);
        }
    }
    if result.trim().is_empty() {
        return Err("谷歌翻译没有返回有效译文".into());
    }
    Ok(result)
}

/// Google returns the detected source language at index 2 of the payload.
pub fn google_detect(text: &str) -> Result<String, String> {
    let value: serde_json::Value = online_http_agent()
        .get(&google_endpoint())
        .query("client", "gtx")
        .query("sl", "auto")
        .query("tl", "en")
        .query("dt", "t")
        .query("q", &text.chars().take(400).collect::<String>())
        .call()
        .map_err(|error| google_request_error(&error))?
        .into_json()
        .map_err(|error| format!("语言检测返回了无效 JSON：{error}"))?;
    value
        .get(2)
        .and_then(|entry| entry.as_str())
        .map(str::to_string)
        .ok_or_else(|| "未能识别原文语言".into())
}

pub fn ollama_models() -> Result<Vec<String>, String> {
    ollama_models_at(&ollama_host())
}

pub fn ollama_pull(model: &str) -> Result<String, String> {
    ollama_pull_with_progress(model, || true, |_| {})
}

pub fn ollama_pull_with_progress(
    model: &str,
    should_continue: impl FnMut() -> bool,
    on_progress: impl FnMut(OllamaPullProgress),
) -> Result<String, String> {
    let model = normalize_ollama_model_reference(model);
    if model.is_empty() {
        return Err("请输入 Ollama 模型名称或模型地址".into());
    }
    ollama_pull_at_with_progress(&ollama_host(), &model, should_continue, on_progress)?;
    Ok(model)
}

fn ollama_pull_at_with_progress(
    host: &str,
    model: &str,
    mut should_continue: impl FnMut() -> bool,
    mut on_progress: impl FnMut(OllamaPullProgress),
) -> Result<(), String> {
    let payload = serde_json::json!({
        "name": model,
        "stream": true,
    });
    let response = ollama_pull_agent()
        .post(&format!("{}/api/pull", normalize_ollama_host(host)))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|error| format!("下载 Ollama 模型 {model} 失败：{error}"))?;

    let started = Instant::now();
    let mut last_progress = started;
    let mut reader = BufReader::new(response.into_reader());
    let mut line = String::new();
    loop {
        if !should_continue() {
            return Err("Ollama 模型下载已取消".into());
        }
        if started.elapsed() >= OLLAMA_PULL_TIMEOUT {
            return Err(format!("下载 Ollama 模型 {model} 超时，已停止"));
        }
        if last_progress.elapsed() >= OLLAMA_PULL_IDLE_TIMEOUT {
            return Err(format!("下载 Ollama 模型 {model} 超时：30 秒没有收到进度"));
        }
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) || error.to_string().to_ascii_lowercase().contains("timed out") =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!("读取 Ollama 模型 {model} 下载进度失败：{error}"));
            }
        };
        if bytes == 0 {
            return Err(format!("下载 Ollama 模型 {model} 未返回完成状态"));
        }
        let value: serde_json::Value = serde_json::from_str(line.trim())
            .map_err(|error| format!("Ollama 下载返回了无效 JSON：{error}"))?;
        if let Some(error) = value.get("error").and_then(|entry| entry.as_str()) {
            return Err(format!("下载 Ollama 模型 {model} 失败：{error}"));
        }
        let progress = OllamaPullProgress {
            status: value
                .get("status")
                .and_then(|entry| entry.as_str())
                .unwrap_or("下载中")
                .to_string(),
            completed: value.get("completed").and_then(|entry| entry.as_u64()),
            total: value.get("total").and_then(|entry| entry.as_u64()),
        };
        let success = progress.status.eq_ignore_ascii_case("success");
        last_progress = Instant::now();
        line.clear();
        on_progress(progress);
        if success {
            return Ok(());
        }
    }
}

pub fn ollama_delete(model: &str) -> Result<String, String> {
    let model = normalize_ollama_model_reference(model);
    if model.is_empty() {
        return Err("没有可删除的 Ollama 模型".into());
    }
    ollama_delete_at(&ollama_host(), &model)?;
    Ok(model)
}

fn ollama_delete_at(host: &str, model: &str) -> Result<(), String> {
    let payload = serde_json::json!({ "name": model });
    let response = ollama_pull_agent()
        .delete(&format!("{}/api/delete", normalize_ollama_host(host)))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|error| format!("删除 Ollama 模型 {model} 失败：{error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("读取 Ollama 删除结果失败：{error}"))?;
    if body.trim().is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("Ollama 删除返回了无效 JSON：{error}"))?;
    if let Some(error) = value.get("error").and_then(|entry| entry.as_str()) {
        return Err(format!("删除 Ollama 模型 {model} 失败：{error}"));
    }
    Ok(())
}

fn ollama_models_at(host: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = http_agent()
        .get(&format!("{}/api/tags", normalize_ollama_host(host)))
        .call()
        .map_err(|error| format!("无法连接本机 Ollama（{host}）：{error}"))?
        .into_json()
        .map_err(|error| format!("Ollama 返回了无效 JSON：{error}"))?;
    let models = value
        .get("models")
        .and_then(|entry| entry.as_array())
        .ok_or("Ollama 返回格式异常")?;
    Ok(models
        .iter()
        .filter_map(|model| model.get("name").and_then(|name| name.as_str()))
        .map(str::to_string)
        .collect())
}

fn ollama_chat(model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    ollama_chat_at(&ollama_host(), model, messages)
}

fn ollama_chat_at(host: &str, model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    let payload = serde_json::json!({
        "model": model,
        "messages": messages
            .iter()
            .map(|message| serde_json::json!({
                "role": message.role,
                "content": message.content,
            }))
            .collect::<Vec<_>>(),
        "stream": false,
        "keep_alive": OLLAMA_MODEL_KEEP_ALIVE,
        "options": {"temperature": 0.2},
    });
    let response = http_agent()
        .post(&format!("{}/api/chat", normalize_ollama_host(host)))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|error| format!("本地模型 {model} 请求失败（请确认 Ollama 已启动）：{error}"))?;
    let value: serde_json::Value = response
        .into_json()
        .map_err(|error| format!("本地模型 {model} 返回了无效 JSON：{error}"))?;
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("本地模型 {model} 没有返回有效译文"))
}

/// Loads the selected Ollama model into memory without generating a translation.
/// Ollama releases it automatically after the normal five-minute idle window.
pub fn ollama_load(model: &str) -> Result<(), String> {
    ollama_load_at(&ollama_host(), model)
}

fn ollama_load_at(host: &str, model: &str) -> Result<(), String> {
    let model = normalize_ollama_model_reference(model);
    if model.is_empty() {
        return Err("未指定 Ollama 模型".into());
    }
    let payload = serde_json::json!({
        "model": model,
        "prompt": "",
        "stream": false,
        "keep_alive": OLLAMA_MODEL_KEEP_ALIVE,
        "options": {"num_predict": 0},
    });
    let response = http_agent()
        .post(&format!("{}/api/generate", normalize_ollama_host(host)))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|error| format!("无法预加载本地模型 {model}（请确认 Ollama 已启动）：{error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("读取本地模型 {model} 预加载结果失败：{error}"))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("本地模型 {model} 预加载返回了无效 JSON：{error}"))?;
    if let Some(error) = value.get("error").and_then(|entry| entry.as_str()) {
        return Err(format!("预加载本地模型 {model} 失败：{error}"));
    }
    Ok(())
}

pub fn ollama_translate(
    model: &str,
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Result<String, String> {
    let source = prompt_source(source);
    let messages = build_messages(text, &source, target, tone, glossary);
    ollama_chat(model, &messages)
}

fn prompt_source(source: &str) -> String {
    if source == "auto" {
        "the source language".into()
    } else {
        source.to_string()
    }
}

fn prompt_for(
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> String {
    let source = prompt_source(source);
    build_messages(text, &source, target, tone, glossary)
        .into_iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Translates by shelling out to the local Codex CLI as a subprocess.
pub fn codex_exec_translate(
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
    model: &str,
) -> Result<String, String> {
    let prompt = prompt_for(text, source, target, tone, glossary);
    let mut command = codex_command()?;
    command.args([
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--color",
        "never",
        "-c",
        "model_reasoning_effort=\"low\"",
    ]);
    if !model.trim().is_empty() {
        command.arg("-m").arg(model.trim());
    }
    command.arg(&prompt);

    execute_codex_command(command)
}

fn codex_search_paths() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let system_paths = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    codex_search_paths_from(
        env::var_os("PATH").as_deref(),
        home.as_deref(),
        &system_paths,
    )
}

fn codex_search_paths_from(
    path: Option<&OsStr>,
    home: Option<&Path>,
    system_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = path.map(env::split_paths).into_iter().flatten().collect();
    if let Some(home) = home {
        paths.extend([
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".codex/bin"),
        ]);
    }
    paths.extend_from_slice(system_paths);
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

fn find_executable(name: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    paths
        .iter()
        .map(|directory| directory.join(name))
        .find(|path| {
            let Ok(metadata) = fs::metadata(path) else {
                return false;
            };
            if !metadata.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        })
}

fn codex_command() -> Result<Command, String> {
    codex_command_in(&codex_search_paths())
}

fn codex_command_in(paths: &[PathBuf]) -> Result<Command, String> {
    let executable = find_executable("codex", paths)
        .ok_or_else(|| "未找到本机 Codex CLI；请先安装并登录 Codex CLI 后重试".to_string())?;
    let child_path = env::join_paths(paths).map_err(|_| "Codex CLI 的 PATH 配置无效")?;
    let mut command = Command::new(executable);
    command.env("PATH", child_path);
    if !proxy_env_configured()
        && let Some(proxy_url) = system_proxy_url()
    {
        for name in [
            "ALL_PROXY",
            "all_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
        ] {
            command.env(name, &proxy_url);
        }
    }
    Ok(command)
}

fn execute_codex_command(command: Command) -> Result<String, String> {
    let (sender, receiver) = mpsc::channel::<Result<String, String>>();
    thread::spawn(move || {
        let result = run_codex_command(command, LLM_OVERALL_TIMEOUT);
        let _ = sender.send(result);
    });
    let stdout = receiver
        .recv_timeout(LLM_OVERALL_TIMEOUT + Duration::from_secs(5))
        .map_err(|_| "Codex CLI 翻译超时".to_string())??;
    let answer = stdout
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty()
                && !trimmed.starts_with('[')
                && !trimmed.starts_with("instructions:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("Codex CLI 没有返回有效译文".into());
    }
    Ok(answer.to_string())
}

fn run_codex_command(mut command: Command, timeout: Duration) -> Result<String, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动本机 Codex CLI：{error}"))?;
    let mut stdout = child.stdout.take().ok_or("无法读取 Codex CLI 输出")?;
    let mut stderr = child.stderr.take().ok_or("无法读取 Codex CLI 错误输出")?;
    let stdout_reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });
    let stderr_reader = thread::spawn(move || {
        let mut output = Vec::new();
        stderr.read_to_end(&mut output).map(|_| output)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("Codex CLI 翻译超时".into());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("检查 Codex CLI 状态失败：{error}"));
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "读取 Codex CLI 输出失败")?
        .map_err(|error| error.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "读取 Codex CLI 错误输出失败")?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        let stdout = String::from_utf8_lossy(&stdout);
        return Err(format!(
            "Codex CLI 执行失败（{status}）：{}{}",
            stderr.trim(),
            if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                String::new()
            }
        ));
    }
    Ok(String::from_utf8_lossy(&stdout).to_string())
}

pub const SILICONFLOW_DEFAULT_ENDPOINT: &str = "https://api.siliconflow.cn/v1/chat/completions";
pub const SILICONFLOW_DEFAULT_MODEL: &str = "Qwen/Qwen3-32B";
const CODEX_MODEL_PRESETS: &[&str] = &[
    "gpt-6-astra",
    "gpt-6-sol",
    "gpt-6.1-sol",
    "gpt-5.6-sol",
    "gpt-5.5",
    "gpt-5.1-codex-mini",
];

/// Returns common Codex model IDs, keeping the configured model even when it
/// is not part of the built-in preset list.
pub fn codex_model_choices(current: &str) -> Vec<String> {
    let mut choices = Vec::new();
    let current = current.trim();
    if !current.is_empty() {
        choices.push(current.to_string());
    }
    for preset in CODEX_MODEL_PRESETS {
        if !choices.iter().any(|choice| choice == preset) {
            choices.push((*preset).to_string());
        }
    }
    choices
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmSettings {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub wire_api: WireApi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireApi {
    ChatCompletions,
    Responses,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmTranslation {
    pub text: String,
    pub used_fallback: bool,
}

impl LlmSettings {
    /// Reads the local Codex CLI config (`~/.codex/auth.json` + `config.toml`).
    pub fn from_codex() -> Result<LlmSettings, String> {
        let home = std::env::var("HOME").map_err(|_| "missing HOME".to_string())?;
        let dir = Path::new(&home).join(".codex");
        let auth = fs::read_to_string(dir.join("auth.json"))
            .map_err(|_| "Codex credentials are missing".to_string())?;
        let config = fs::read_to_string(dir.join("config.toml")).unwrap_or_default();
        from_codex_files(&auth, &config)
    }

    pub fn discover_codex_with_model(model: &str) -> Result<LlmSettings, String> {
        let mut settings = LlmSettings::from_codex()?;
        apply_model_override(&mut settings, model);
        Ok(settings)
    }

    pub fn discover_with_model(model: &str) -> Result<LlmSettings, String> {
        Self::discover_codex_with_model(model)
    }
}

/// Applies a saved model override; an empty override keeps the discovered model.
pub fn apply_model_override(settings: &mut LlmSettings, model: &str) {
    let model = model.trim();
    if !model.is_empty() {
        settings.model = model.to_string();
    }
}

/// Builds settings for the SiliconFlow-compatible OpenAI chat endpoint.
pub fn siliconflow_settings(
    api_key: &str,
    endpoint: &str,
    model: &str,
) -> Result<LlmSettings, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("SiliconFlow API Key is missing".to_string());
    }
    let endpoint = endpoint.trim();
    let endpoint = if endpoint.is_empty() {
        SILICONFLOW_DEFAULT_ENDPOINT.to_string()
    } else if endpoint.ends_with("/chat/completions") {
        endpoint.to_string()
    } else {
        format!("{}/chat/completions", endpoint.trim_end_matches('/'))
    };
    let model = {
        let value = model.trim();
        if value.is_empty() {
            SILICONFLOW_DEFAULT_MODEL.to_string()
        } else {
            value.to_string()
        }
    };
    Ok(LlmSettings {
        endpoint,
        model,
        api_key: key.to_string(),
        wire_api: WireApi::ChatCompletions,
    })
}

/// Parses Codex CLI auth/config files without ever printing the secret.
pub fn from_codex_files(auth: &str, config: &str) -> Result<LlmSettings, String> {
    let auth: serde_json::Value =
        serde_json::from_str(auth).map_err(|_| "Codex credentials are missing".to_string())?;
    let api_key = auth
        .get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if api_key.is_empty() {
        return Err("Codex credentials are missing".to_string());
    }

    let mut model = String::new();
    let mut base_url = String::new();
    let mut wire_api = String::new();
    let mut in_provider_table = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[model_providers.") {
            in_provider_table = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_provider_table = false;
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "model" if !in_provider_table => model = value,
            "base_url" if in_provider_table => base_url = value,
            "wire_api" if in_provider_table => wire_api = value,
            _ => {}
        }
    }
    if base_url.is_empty() {
        return Err("Codex config is missing a base_url".to_string());
    }
    let wire = if wire_api == "responses" {
        WireApi::Responses
    } else {
        WireApi::ChatCompletions
    };
    let endpoint = if wire == WireApi::Responses {
        format!("{}/responses", base_url.trim_end_matches('/'))
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    };
    Ok(LlmSettings {
        endpoint,
        model,
        api_key,
        wire_api: wire,
    })
}

/// Builds the JSON request body for the configured wire API.
pub fn build_request(
    settings: &LlmSettings,
    source: &str,
    target: &str,
    text: &str,
) -> serde_json::Value {
    let system = format!(
        "Translate from {source} to {target}. Return only the translation. Preserve the exact meaning, negation, conditions, amounts, dates, quantities, names and product identifiers."
    );
    match settings.wire_api {
        WireApi::ChatCompletions => serde_json::json!({
            "model": settings.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": text},
            ],
            "stream": false,
        }),
        WireApi::Responses => serde_json::json!({
            "model": settings.model,
            "input": [
                {"role": "system", "content": system},
                {"role": "user", "content": text},
            ],
            "stream": false,
        }),
    }
}

/// Extracts the translation text or a provider error message from a response.
pub fn parse_response(body: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("invalid response: {error}"))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("request failed");
        return Err(message.to_string());
    }
    if let Some(text) = value.get("output_text").and_then(|text| text.as_str()) {
        let text = text.trim();
        if !text.is_empty() {
            return Ok(text.to_string());
        }
    }
    let content = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::trim)
        .unwrap_or("");
    if content.is_empty() {
        return Err("模型未返回翻译内容".to_string());
    }
    Ok(content.to_string())
}

fn request_error(status: u16, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
    {
        return message.to_string();
    }
    format!("HTTP {status}")
}

/// Sends one HTTP translation request and parses the reply.
pub fn translate_with_profile(
    settings: &LlmSettings,
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Result<String, String> {
    let mut request = build_request(settings, source, target, text);
    if let Some(object) = request.as_object_mut() {
        let prompt = prompt_for(text, source, target, tone, glossary);
        // Keep the wire format identical, but carry the glossary context in the
        // user message when a glossary is configured.
        if !glossary.is_empty() {
            let key = if settings.wire_api == WireApi::Responses {
                "input"
            } else {
                "messages"
            };
            if let Some(entries) = object.get_mut(key).and_then(|v| v.as_array_mut())
                && let Some(last) = entries.last_mut()
                && let Some(content) = last.get_mut("content")
            {
                *content =
                    serde_json::Value::String(prompt.lines().last().unwrap_or(text).to_string());
            }
        }
    }
    let payload = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    let response = online_http_agent()
        .post(&settings.endpoint)
        .set("Authorization", &format!("Bearer {}", settings.api_key))
        .set("Content-Type", "application/json")
        .send_string(&payload)
        .map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let body = response.into_string().unwrap_or_default();
                request_error(status, &body)
            }
            other => other.to_string(),
        })?;
    let body = response.into_string().map_err(|error| error.to_string())?;
    parse_response(&body)
}

/// Translates with a primary online profile and an OpenAI/Codex fallback.
#[allow(clippy::too_many_arguments)]
pub fn translate_with_fallback(
    provider_label: &str,
    primary: Result<LlmSettings, String>,
    fallback: impl FnOnce() -> Result<LlmSettings, String>,
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Result<LlmTranslation, String> {
    match primary {
        Ok(settings) => {
            match translate_with_profile(&settings, text, source, target, tone, glossary) {
                Ok(text) => Ok(LlmTranslation {
                    text,
                    used_fallback: false,
                }),
                Err(primary_error) => match fallback() {
                    Ok(fallback_settings) => {
                        let text = translate_codex_with_profile(
                            &fallback_settings,
                            text,
                            source,
                            target,
                            tone,
                            glossary,
                        )?;
                        Ok(LlmTranslation {
                            text,
                            used_fallback: true,
                        })
                    }
                    Err(fallback_error) => Err(format!(
                        "{provider_label} 翻译失败：{primary_error}；Codex 也不可用：{fallback_error}"
                    )),
                },
            }
        }
        Err(primary_error) => {
            // Missing credentials etc.: try Codex without contacting the primary.
            let fallback_settings = fallback().map_err(|fallback_error| {
                format!("{provider_label}：{primary_error}；Codex 也不可用：{fallback_error}")
            })?;
            let text = translate_codex_with_profile(
                &fallback_settings,
                text,
                source,
                target,
                tone,
                glossary,
            )
            .map_err(|fallback_error| {
                format!("{provider_label}：{primary_error}；Codex 兜底失败：{fallback_error}")
            })?;
            Ok(LlmTranslation {
                text,
                used_fallback: true,
            })
        }
    }
}

/// Codex-compatible HTTP translation used for the fallback profile.
pub fn translate_codex_with_profile(
    settings: &LlmSettings,
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Result<String, String> {
    translate_with_profile(settings, text, source, target, tone, glossary)
        .map_err(|error| format!("本机 Codex 不可用：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        Tone, codex_command_in, codex_model_choices, execute_codex_command, google_request_error,
        normalize_ollama_host, normalize_ollama_model_reference, ollama_chat_at, ollama_delete_at,
        ollama_load_at, ollama_models_at, ollama_pull_at_with_progress, parse_system_proxy_output,
        prompt_for, run_codex_command,
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        time::{Duration, Instant},
    };

    #[test]
    fn codex_subprocess_returns_stdout() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'translated output\\n'"]);
        assert_eq!(execute_codex_command(command).unwrap(), "translated output");
    }

    #[test]
    fn codex_cli_is_found_outside_the_gui_path_and_receives_its_runtime_path() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "easytranslate-codex-path-{}-{nonce}",
            std::process::id()
        ));
        let bin = root.join("homebrew/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = bin.join("codex");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$PATH"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut command = codex_command_in(std::slice::from_ref(&bin)).unwrap();
        command.args(["exec", "test"]);
        let output = execute_codex_command(command).unwrap();

        assert!(
            output
                .split(':')
                .any(|entry| entry == bin.to_str().unwrap())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_model_choices_keep_custom_current_model_and_presets() {
        let choices = codex_model_choices("vendor-fast-model");
        assert_eq!(choices[0], "vendor-fast-model");
        assert!(choices.iter().any(|choice| choice == "gpt-6-sol"));
        assert_eq!(
            choices
                .iter()
                .filter(|choice| *choice == "gpt-6-sol")
                .count(),
            1
        );
    }

    #[test]
    fn google_connection_error_does_not_echo_request_url_or_source_text() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let error = ureq::get(&format!("http://{address}/translate"))
            .query("q", "private source text")
            .call()
            .unwrap_err();
        let message = google_request_error(&error);

        assert!(!message.contains("private source text"));
        assert!(!message.contains("q="));
        assert!(message.contains("网络") || message.contains("连接"));
    }

    #[test]
    fn timed_out_codex_subprocess_is_killed() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 2"]);
        let started = Instant::now();
        let error = run_codex_command(command, Duration::from_millis(30)).unwrap_err();
        assert!(error.contains("超时"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn automatic_prompt_does_not_send_text_to_google_detection() {
        let prompt = prompt_for("你好", "auto", "en", Tone::Faithful, &[]);
        assert!(prompt.contains("the source language"));
    }

    #[test]
    fn system_proxy_parser_prefers_enabled_https_proxy() {
        let output = r#"
        HTTPEnable : 1
        HTTPProxy : 127.0.0.1
        HTTPPort : 8080
        HTTPSEnable : 1
        HTTPSProxy : 127.0.0.1
        HTTPSPort : 7890
        SOCKSEnable : 1
        SOCKSProxy : 127.0.0.1
        SOCKSPort : 7891
        "#;

        assert_eq!(
            parse_system_proxy_output(output),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn system_proxy_parser_ignores_disabled_or_incomplete_entries() {
        let output = r#"
        HTTPEnable : 0
        HTTPProxy : 127.0.0.1
        HTTPPort : 8080
        HTTPSEnable : 1
        HTTPSProxy :
        HTTPSPort : 7890
        SOCKSEnable : 0
        SOCKSProxy : 127.0.0.1
        SOCKSPort : 7891
        "#;

        assert_eq!(parse_system_proxy_output(output), None);
    }

    #[test]
    fn ollama_host_is_normalized_for_native_api_paths() {
        assert_eq!(
            normalize_ollama_host("http://127.0.0.1:11434"),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            normalize_ollama_host("127.0.0.1:11434/v1/"),
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn ollama_model_reference_accepts_names_and_registry_urls() {
        assert_eq!(normalize_ollama_model_reference("qwen2.5:3b"), "qwen2.5:3b");
        assert_eq!(
            normalize_ollama_model_reference("https://ollama.com/library/qwen2.5:3b"),
            "qwen2.5:3b"
        );
        assert_eq!(
            normalize_ollama_model_reference("https://huggingface.co/unsloth/model"),
            "hf.co/unsloth/model"
        );
    }

    #[test]
    fn ollama_preload_requests_load_without_generating_output_and_sets_keep_alive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..count]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or_default();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request);
            assert!(
                request.starts_with("POST /api/generate HTTP/1.1"),
                "{request}"
            );
            let body = request.split_once("\r\n\r\n").unwrap().1;
            let payload: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(payload["model"], "translategemma:latest");
            assert_eq!(payload["prompt"], "");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["keep_alive"], "5m");
            assert_eq!(payload["options"]["num_predict"], 0);
            let response = r#"{"done":true,"response":""}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        ollama_load_at(&format!("http://{address}"), "translategemma:latest").unwrap();
        server.join().unwrap();
    }

    #[test]
    fn ollama_translation_keeps_its_model_loaded_for_five_idle_minutes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..count]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or_default();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("POST /api/chat HTTP/1.1"), "{request}");
            let body = request.split_once("\r\n\r\n").unwrap().1;
            let payload: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(payload["model"], "translategemma:latest");
            assert_eq!(payload["keep_alive"], "5m");
            let response = r#"{"message":{"content":"Please wait."}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        let messages = [crate::local_ai::ChatMessage {
            role: "user".into(),
            content: "请稍等。".into(),
        }];
        assert_eq!(
            ollama_chat_at(
                &format!("http://{address}"),
                "translategemma:latest",
                &messages
            )
            .unwrap(),
            "Please wait."
        );
        server.join().unwrap();
    }

    #[test]
    fn ollama_discovery_uses_native_api_path_even_for_v1_host() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /api/tags HTTP/1.1"), "{request}");
            let body = r#"{"models":[{"name":"qwen3:8b"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let models = ollama_models_at(&format!("http://{address}/v1")).unwrap();
        server.join().unwrap();
        assert_eq!(models, ["qwen3:8b"]);
    }

    #[test]
    fn ollama_pull_and_delete_use_native_api_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for expected in [("POST", "/api/pull"), ("DELETE", "/api/delete")] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    request.extend_from_slice(&chunk[..count]);
                    let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let header_end = header_end + 4;
                    let content_length = String::from_utf8_lossy(&request[..header_end])
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or_default();
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with(&format!("{} {} HTTP/1.1", expected.0, expected.1)));
                assert!(request.contains("qwen2.5:3b"));
                let body = r#"{"status":"success"}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        ollama_pull_at_with_progress(&format!("http://{address}"), "qwen2.5:3b", || true, |_| {})
            .unwrap();
        ollama_delete_at(&format!("http://{address}"), "qwen2.5:3b").unwrap();
        server.join().unwrap();
    }

    #[test]
    fn ollama_pull_stream_reports_progress_and_completion() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..count]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or_default();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("POST /api/pull HTTP/1.1"), "{request}");
            assert!(request.contains("\"stream\":true"), "{request}");
            let body = concat!(
                r#"{"status":"pulling manifest"}
"#,
                r#"{"status":"downloading","completed":512,"total":1024}
"#,
                r#"{"status":"success"}
"#,
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let mut progress = Vec::new();
        ollama_pull_at_with_progress(
            &format!("http://{address}"),
            "translategemma",
            || true,
            |item| progress.push(item),
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(progress.len(), 3);
        assert_eq!(progress[0].status, "pulling manifest");
        assert_eq!(progress[1].completed, Some(512));
        assert_eq!(progress[1].total, Some(1024));
        assert_eq!(progress[2].status, "success");
    }
}
