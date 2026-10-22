use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize, Clone)]
pub struct Request {
    pub action: String,
    pub text: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
    pub path: Option<String>,
}
#[derive(Deserialize, Debug)]
pub struct Response {
    pub text: Option<String>,
    pub error: Option<String>,
    pub status: Option<String>,
}
pub fn decode(bytes: &[u8]) -> Result<Response, String> {
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| "系统翻译返回格式错误".to_string())?;
    if let Some(error) = &response.error {
        return Err(error.clone());
    }
    if response.text.as_ref().is_none_or(|s| s.trim().is_empty()) && response.status.is_none() {
        return Err("系统未返回有效译文".into());
    }
    Ok(response)
}
pub fn executable() -> PathBuf {
    let adjacent = std::env::current_exe()
        .unwrap_or_default()
        .with_file_name("TranslationBridge");
    if adjacent.exists() {
        adjacent
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".build/TranslationBridge")
    }
}
pub fn run(request: Request) -> Result<Response, String> {
    run_cancellable(request, None)
}
pub fn run_cancellable(
    request: Request,
    cancellation: Option<(Arc<AtomicU64>, u64)>,
) -> Result<Response, String> {
    let cancelled = || {
        cancellation
            .as_ref()
            .is_some_and(|(token, generation)| token.load(Ordering::Relaxed) != *generation)
    };
    if cancelled() {
        return Err("翻译已取消".into());
    }
    let mut child = Command::new(executable())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动系统翻译桥接，请先构建应用：{e}"))?;
    let bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    if let Err(e) = child
        .stdin
        .take()
        .ok_or("无法打开翻译输入")?
        .write_all(&bytes)
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.to_string());
    }
    let mut stdout = child.stdout.take().ok_or("无法读取翻译输出")?;
    let reader = thread::spawn(move || {
        let mut data = Vec::new();
        stdout.read_to_end(&mut data).map(|_| data)
    });
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("翻译已取消".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader
                    .join()
                    .map_err(|_| "翻译输出读取失败")?
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("翻译或识别进程异常退出，请重试。".into());
                }
                return decode(&bytes);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err("翻译或识别超时，请重试；首次识别初始化可能较慢。".into());
            }
        }
    }
}
pub fn setup(source: &str, target: &str, sample: &str) -> Result<(), String> {
    let request = Request {
        action: "setup".into(),
        text: Some(sample.into()),
        source: Some(source.into()),
        target: Some(target.into()),
        path: None,
    };
    let mut child = Command::new(executable())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("无法启动语言包设置")?
        .write_all(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
