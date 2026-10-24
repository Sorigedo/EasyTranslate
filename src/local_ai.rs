use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

pub trait TranslationRoute {
    fn translate_long(&self, text: &str) -> String;
}

pub trait RouteLoader: Send + Sync {
    fn load(&self, source: &str, target: &str) -> Result<Box<dyn TranslationRoute>, String>;
}

pub struct LocalTranslator {
    requests: mpsc::Sender<TranslationRequest>,
}

impl LocalTranslator {
    pub fn default_cache_root() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/EasyTranslate/Models")
    }

    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self::with_loader(Arc::new(FxRouteLoader {
            cache: fxtranslate::cache::Cache::with_root(cache_root),
        }))
    }

    pub fn with_loader(loader: Arc<dyn RouteLoader>) -> Self {
        let (requests, incoming) = mpsc::channel::<TranslationRequest>();
        thread::Builder::new()
            .name("easytranslate-models".into())
            .spawn(move || {
                let mut routes: HashMap<(String, String), Box<dyn TranslationRoute>> =
                    HashMap::new();
                while let Ok(request) = incoming.recv() {
                    let TranslationRequest {
                        text,
                        source,
                        target,
                        response,
                        cancellation,
                        preload,
                    } = request;
                    if cancellation.as_ref().is_some_and(|(token, generation)| {
                        token.load(std::sync::atomic::Ordering::Relaxed) != *generation
                    }) {
                        let _ = response.send(Err("翻译已取消".into()));
                        continue;
                    }
                    let route_key = (source.clone(), target.clone());
                    let result = if source == target {
                        Ok(text)
                    } else {
                        if !routes.contains_key(&route_key) {
                            match loader.load(&source, &target) {
                                Ok(route) => {
                                    routes.insert(route_key.clone(), route);
                                }
                                Err(error) => {
                                    let _ = response.send(Err(error));
                                    continue;
                                }
                            }
                        }
                        if cancellation.as_ref().is_some_and(|(token, generation)| {
                            token.load(std::sync::atomic::Ordering::Relaxed) != *generation
                        }) {
                            let _ = response.send(Err("翻译已取消".into()));
                            continue;
                        }
                        if preload {
                            Ok(String::new())
                        } else {
                            Ok(routes
                                .get(&route_key)
                                .expect("route inserted above")
                                .translate_long(&text))
                        }
                    };
                    let _ = response.send(result);
                }
            })
            .expect("failed to start local translation worker");
        Self { requests }
    }

    pub fn translate(&self, text: &str, source: &str, target: &str) -> Result<String, String> {
        self.translate_cancellable(text, source, target, None)
    }

    pub fn preload(&self, source: &str, target: &str) -> Result<(), String> {
        let (response, result) = mpsc::channel();
        self.requests
            .send(TranslationRequest {
                text: String::new(),
                source: source.into(),
                target: target.into(),
                response,
                cancellation: None,
                preload: true,
            })
            .map_err(|_| "本地翻译工作线程已停止".to_string())?;
        result
            .recv()
            .map_err(|_| "本地翻译工作线程未返回模型状态".to_string())?
            .map(|_| ())
    }

    pub fn translate_cancellable(
        &self,
        text: &str,
        source: &str,
        target: &str,
        cancellation: Option<(Arc<std::sync::atomic::AtomicU64>, u64)>,
    ) -> Result<String, String> {
        let (response, result) = mpsc::channel();
        self.requests
            .send(TranslationRequest {
                text: text.to_string(),
                source: source.to_string(),
                target: target.to_string(),
                response,
                cancellation: cancellation.clone(),
                preload: false,
            })
            .map_err(|_| "本地翻译工作线程已停止".to_string())?;
        loop {
            if cancellation.as_ref().is_some_and(|(token, generation)| {
                token.load(std::sync::atomic::Ordering::Relaxed) != *generation
            }) {
                return Err("翻译已取消".into());
            }
            match result.recv_timeout(Duration::from_millis(30)) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("本地翻译工作线程未返回结果".into());
                }
            }
        }
    }
}

struct TranslationRequest {
    text: String,
    source: String,
    target: String,
    response: mpsc::Sender<Result<String, String>>,
    cancellation: Option<(Arc<std::sync::atomic::AtomicU64>, u64)>,
    preload: bool,
}

struct FxRouteLoader {
    cache: fxtranslate::cache::Cache,
}

impl RouteLoader for FxRouteLoader {
    fn load(&self, source: &str, target: &str) -> Result<Box<dyn TranslationRoute>, String> {
        let translation = fxtranslate::loader::load_translation(
            &fxtranslate::fetch::NetworkFetch::new(),
            &self.cache,
            source,
            target,
        )
        .map_err(|error| format!("无法准备 {source} → {target} 本地翻译模型：{error}"))?;
        Ok(Box::new(FxTranslationRoute(translation)))
    }
}

struct FxTranslationRoute(fxtranslate::engine::Translation);

impl TranslationRoute for FxTranslationRoute {
    fn translate_long(&self, text: &str) -> String {
        self.0.translate_long(text)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    CustomerService,
    BusinessFormal,
    Friendly,
    Concise,
    Faithful,
}

impl Tone {
    pub fn label(self) -> &'static str {
        match self {
            Self::CustomerService => "礼貌专业客服",
            Self::BusinessFormal => "正式商务",
            Self::Friendly => "自然友好",
            Self::Concise => "简洁直接",
            Self::Faithful => "忠实原文",
        }
    }

    fn instruction(self, target: &str) -> &'static str {
        match (self, target.starts_with("zh")) {
            (Self::CustomerService, false) => {
                "Use courteous, professional customer-support language addressed to the customer."
            }
            (Self::CustomerService, true) => "使用面向客户的礼貌、专业客服表达。",
            (Self::BusinessFormal, false) => "Use formal, polished business language.",
            (Self::BusinessFormal, true) => "使用正式、严谨的商务表达。",
            (Self::Friendly, false) => "Use a natural, warm and friendly tone.",
            (Self::Friendly, true) => "使用自然、温和、友好的表达。",
            (Self::Concise, false) => "Use concise, direct language without unnecessary words.",
            (Self::Concise, true) => "表达简洁直接，不添加多余内容。",
            (Self::Faithful, false) => "Stay close to the original wording and register.",
            (Self::Faithful, true) => "忠实原文措辞和语体。",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
    pub source_language: String,
    pub target_language: String,
    pub note: String,
    pub enabled: bool,
}

impl Default for GlossaryEntry {
    fn default() -> Self {
        Self {
            source: String::new(),
            target: String::new(),
            source_language: "zh-Hans".into(),
            target_language: "en".into(),
            note: String::new(),
            enabled: true,
        }
    }
}

impl GlossaryEntry {
    pub fn new(source: &str, target: &str, source_language: &str, target_language: &str) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            source_language: source_language.into(),
            target_language: target_language.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub fn seeded_glossary() -> Vec<GlossaryEntry> {
    [
        ("固件", "firmware"),
        ("驱动程序", "device driver"),
        ("恢复出厂设置", "factory reset"),
        ("故障排查", "troubleshooting"),
        ("保修期", "warranty period"),
        ("设备管理器", "Device Manager"),
        ("操作系统", "operating system"),
        ("网络适配器", "network adapter"),
        ("安装程序", "installer"),
        ("许可证", "license"),
    ]
    .into_iter()
    .flat_map(|(zh, en)| {
        [
            GlossaryEntry::new(zh, en, "zh-Hans", "en"),
            GlossaryEntry::new(en, zh, "en", "zh-Hans"),
        ]
    })
    .collect()
}

pub fn matching_terms(
    text: &str,
    source: &str,
    target: &str,
    glossary: &[GlossaryEntry],
) -> Vec<GlossaryEntry> {
    let folded_text = text.to_lowercase();
    let mut matches: Vec<_> = glossary
        .iter()
        .filter(|entry| {
            entry.enabled
                && !entry.source.trim().is_empty()
                && !entry.target.trim().is_empty()
                && entry.source_language == source
                && entry.target_language == target
                && folded_text.contains(&entry.source.to_lowercase())
        })
        .cloned()
        .collect();
    matches.sort_by(|a, b| {
        b.source
            .chars()
            .count()
            .cmp(&a.source.chars().count())
            .then_with(|| a.source.cmp(&b.source))
    });
    matches.dedup_by(|a, b| {
        a.source.eq_ignore_ascii_case(&b.source)
            && a.source_language == b.source_language
            && a.target_language == b.target_language
    });
    matches
}

/// Put configured terminology into the source text before it reaches Marian.
///
/// Marian is a translation model rather than a prompt-following model, so a
/// glossary cannot be passed as a system instruction. Replacing a matched
/// source term with its requested target spelling is deterministic and keeps
/// the rest of the sentence available to the model for translation.
pub fn apply_glossary_to_source(
    text: &str,
    source: &str,
    target: &str,
    glossary: &[GlossaryEntry],
) -> String {
    matching_terms(text, source, target, glossary)
        .into_iter()
        .fold(text.to_string(), |value, entry| {
            replace_term(&value, &entry.source, &entry.target, source)
        })
}

/// Repair a source term that a model copied verbatim instead of translating.
/// This is intentionally conservative: it only replaces configured source
/// terms, never guesses synonyms or rewrites unrelated output.
pub fn apply_glossary_to_result(
    text: &str,
    source: &str,
    target: &str,
    glossary: &[GlossaryEntry],
) -> String {
    matching_terms(text, source, target, glossary)
        .into_iter()
        .fold(text.to_string(), |value, entry| {
            replace_term(&value, &entry.source, &entry.target, source)
        })
}

fn replace_term(text: &str, source_term: &str, target_term: &str, language: &str) -> String {
    if language.starts_with("en") && source_term.is_ascii() {
        let mut result = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(index) = rest.char_indices().find_map(|(index, _)| {
            rest.get(index..index + source_term.len())
                .filter(|candidate| candidate.eq_ignore_ascii_case(source_term))
                .map(|_| index)
        }) {
            result.push_str(&rest[..index]);
            result.push_str(target_term);
            rest = &rest[index + source_term.len()..];
        }
        result.push_str(rest);
        result
    } else {
        text.replace(source_term, target_term)
    }
}

pub fn build_messages(
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Vec<ChatMessage> {
    let mut system = format!(
        "Translate from {source} to {target}. Return only the translation, with no explanation. {} Preserve the exact meaning, negation, conditions, amounts, dates, quantities, names, product identifiers, paragraph breaks, uncertainty and troubleshooting steps. Do not invent facts, promises, apologies or extra advice. Never omit or reverse a negation. {}",
        tone.instruction(target),
        "事实保护要求：否定、条件、金额、日期、数量、专名和不确定程度必须保留；不得补充原文没有的承诺、歉意或事实。"
    );
    let terms = matching_terms(text, source, target, glossary);
    if !terms.is_empty() {
        system.push_str("\nPreferred glossary terms (source → required translation):");
        for entry in terms {
            system.push_str(&format!("\n{} → {}", entry.source, entry.target));
            if !entry.note.trim().is_empty() {
                system.push_str(&format!(" ({})", entry.note.trim()));
            }
        }
        system.push_str(
            "\nUse these exact translations where the terms occur; do not alter surrounding facts.",
        );
    }
    vec![
        ChatMessage {
            role: "system".into(),
            content: system,
        },
        ChatMessage {
            role: "user".into(),
            content: text.into(),
        },
    ]
}

/// Builds the shorter prompt used by a local Ollama model. Ollama already
/// receives the translation task as a structured chat request, so repeating
/// the full safety guidance in two languages wastes prompt evaluation time.
/// Keep the same user text and the constraints that affect translation
/// correctness, while leaving the richer prompt available to online models.
pub fn build_ollama_messages(
    text: &str,
    source: &str,
    target: &str,
    tone: Tone,
    glossary: &[GlossaryEntry],
) -> Vec<ChatMessage> {
    let mut system = format!(
        "Translate from {source} to {target}. Output only the translation. Preserve meaning, negation, conditions, amounts, dates, quantities, names, product IDs and line breaks. Do not add facts or explanations. {}",
        tone.instruction(target)
    );
    let terms = matching_terms(text, source, target, glossary);
    if !terms.is_empty() {
        system.push_str(" Glossary (source → translation):");
        for entry in terms {
            system.push_str(&format!(" {} → {}", entry.source, entry.target));
        }
    }
    vec![
        ChatMessage {
            role: "system".into(),
            content: system,
        },
        ChatMessage {
            role: "user".into(),
            content: text.into(),
        },
    ]
}

pub fn profile_key(tone: Tone, glossary: &[GlossaryEntry]) -> String {
    let serialized = serde_json::to_vec(&(tone, glossary)).unwrap_or_default();
    let hash = serialized.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{}:{hash:016x}", tone.label())
}
