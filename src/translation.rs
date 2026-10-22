use crate::{
    article,
    bridge::{self, Request},
    cache::{Cache, Key},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    LocalAi,
    Google,
    OpenAiCli,
    LocalModel,
    SiliconFlow,
    Llm,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalAi => "本地开源模型",
            Self::Google => "谷歌翻译",
            Self::OpenAiCli | Self::Llm => "OpenAI / 本机 Codex",
            Self::LocalModel => "本地模型",
            Self::SiliconFlow => "硅基流动",
        }
    }
}

#[derive(Clone)]
pub struct Options {
    pub provider: Provider,
    pub online_enabled: bool,
    pub siliconflow_endpoint: String,
    pub siliconflow_model: String,
    pub siliconflow_api_key: String,
    pub codex_model: String,
    pub local_translator: Option<Arc<crate::local_ai::LocalTranslator>>,
    pub tone: crate::local_ai::Tone,
    pub glossary: Vec<crate::local_ai::GlossaryEntry>,
}

impl Options {
    pub fn check(&self) -> Result<(), String> {
        match self.provider {
            Provider::LocalAi if self.local_translator.is_none() => {
                Err("本地翻译服务尚未启动。".into())
            }
            Provider::LocalAi | Provider::LocalModel => Ok(()),
            _ if !self.online_enabled => Err("请先启用联网翻译授权。".into()),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslationResult {
    pub text: String,
    pub used_fallback: bool,
}

pub fn run(
    options: Options,
    text: &str,
    source: &str,
    target: &str,
    cancellation: (Arc<AtomicU64>, u64),
    cache: Arc<Mutex<Cache>>,
    mut progress: impl FnMut(usize, usize),
) -> Result<TranslationResult, String> {
    options.check()?;
    let cancelled = || cancellation.0.load(Ordering::Relaxed) != cancellation.1;
    let check = || {
        if cancelled() {
            Err("翻译已取消".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    if text.trim().is_empty() {
        return Err("请输入文字".into());
    }
    let source = if source == "auto" {
        bridge::run_cancellable(
            Request {
                action: "detect".into(),
                text: Some(text.into()),
                source: None,
                target: None,
                path: None,
            },
            Some(cancellation.clone()),
        )?
        .text
        .ok_or("未识别出原文语言")?
    } else {
        source.to_string()
    };
    let llm_parts = matches!(
        options.provider,
        Provider::OpenAiCli | Provider::Llm | Provider::SiliconFlow
    );
    let parts = if llm_parts {
        article::split_llm(text, 12000)
    } else {
        article::split(text, 1500)
    };
    let total = parts
        .iter()
        .filter(|p| matches!(p, article::Part::Translate(_)))
        .count();
    let mut done = 0;
    progress(0, total);
    let mut result = String::new();
    for part in parts {
        match part {
            article::Part::Whitespace(value) => result.push_str(&value),
            article::Part::Translate(value) => {
                check()?;
                let key = Key {
                    source: cache_source_key(options.provider, &options.codex_model, &source),
                    target: target.into(),
                    text: value.clone(),
                    profile: crate::local_ai::profile_key(options.tone, &options.glossary),
                };
                let translated = cache.lock().ok().and_then(|mut c| c.get(&key));
                let translated = match translated {
                    Some(v) => v,
                    None => {
                        let v = translate_part(&options, &value, &source, target)?;
                        if let Ok(mut c) = cache.lock() {
                            c.put(key, v.clone());
                        }
                        v
                    }
                };
                result.push_str(&translated);
                done += 1;
                progress(done, total);
            }
        }
    }
    Ok(TranslationResult {
        text: result,
        used_fallback: false,
    })
}

fn cache_source_key(provider: Provider, model: &str, source: &str) -> String {
    let provider = match provider {
        Provider::LocalAi => "local_ai".to_string(),
        Provider::LocalModel => format!("local_model:{model}"),
        Provider::Google => "google".to_string(),
        Provider::OpenAiCli | Provider::Llm => "openai_cli".to_string(),
        Provider::SiliconFlow => "silicon_flow".to_string(),
    };
    format!("{provider}:{source}")
}

fn translate_part(
    options: &Options,
    text: &str,
    source: &str,
    target: &str,
) -> Result<String, String> {
    match options.provider {
        Provider::Google => crate::llm::google_translate(text, source, target),
        Provider::OpenAiCli | Provider::Llm => crate::llm::codex_exec_translate(
            text,
            source,
            target,
            options.tone,
            &options.glossary,
            &options.codex_model,
        ),
        Provider::LocalModel => crate::llm::ollama_translate(
            &options.codex_model,
            text,
            source,
            target,
            options.tone,
            &options.glossary,
        ),
        Provider::SiliconFlow => {
            let settings = crate::llm::siliconflow_settings(
                &options.siliconflow_api_key,
                &options.siliconflow_endpoint,
                &options.siliconflow_model,
            )?;
            crate::llm::translate_with_profile(
                &settings,
                text,
                source,
                target,
                options.tone,
                &options.glossary,
            )
        }
        Provider::LocalAi => {
            let prepared =
                crate::local_ai::apply_glossary_to_source(text, source, target, &options.glossary);
            let translated = options
                .local_translator
                .as_ref()
                .ok_or("本地翻译服务尚未启动")?
                .translate_cancellable(&prepared, source, target, None)?;
            Ok(crate::local_ai::apply_glossary_to_result(
                &translated,
                source,
                target,
                &options.glossary,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Provider, cache_source_key};

    #[test]
    fn ollama_model_name_is_part_of_translation_cache_identity() {
        let qwen = cache_source_key(Provider::LocalModel, "qwen3:8b", "zh-Hans");
        let llama = cache_source_key(Provider::LocalModel, "llama3.2", "zh-Hans");

        assert_ne!(qwen, llama);
        assert_eq!(
            cache_source_key(Provider::LocalAi, "", "zh-Hans"),
            "local_ai:zh-Hans"
        );
    }
}
