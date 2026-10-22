use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    LocalAi,
    Ollama,
    Google,
    OpenAiCli,
    OpenAiCompatible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

impl ServiceConfig {
    pub fn provider(&self) -> crate::translation::Provider {
        match self.kind {
            ServiceKind::LocalAi => crate::translation::Provider::LocalAi,
            ServiceKind::Ollama => crate::translation::Provider::LocalModel,
            ServiceKind::Google => crate::translation::Provider::Google,
            ServiceKind::OpenAiCli => crate::translation::Provider::Llm,
            ServiceKind::OpenAiCompatible => crate::translation::Provider::SiliconFlow,
        }
    }

    pub fn model_for_translation<'a>(&'a self, codex_model: &'a str) -> &'a str {
        match self.kind {
            ServiceKind::Ollama => self.model.trim(),
            ServiceKind::OpenAiCli if !self.model.trim().is_empty() => self.model.trim(),
            _ => codex_model.trim(),
        }
    }

    pub fn local(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ServiceKind::LocalAi,
            endpoint: String::new(),
            model: String::new(),
            api_key: String::new(),
        }
    }

    pub fn ollama(model: &str) -> Self {
        Self {
            id: format!("ollama:{model}"),
            name: format!("Ollama · {model}"),
            kind: ServiceKind::Ollama,
            endpoint: String::new(),
            model: model.into(),
            api_key: String::new(),
        }
    }

    pub fn openai_cli(id: &str, name: &str, model: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ServiceKind::OpenAiCli,
            endpoint: String::new(),
            model: model.into(),
            api_key: String::new(),
        }
    }

    pub fn compatible(id: &str, name: &str, endpoint: &str, model: &str, api_key: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ServiceKind::OpenAiCompatible,
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: api_key.into(),
        }
    }
}

/// Keeps one selectable service for every model currently installed in Ollama.
pub fn sync_ollama_models(services: &mut Vec<ServiceConfig>, models: &[String]) {
    services.retain(|service| service.kind != ServiceKind::Ollama);
    let mut seen = std::collections::HashSet::new();
    let insert_at = services
        .iter()
        .rposition(|service| service.kind == ServiceKind::LocalAi)
        .map_or(0, |index| index + 1);
    let mut ollama_services = Vec::new();
    for model in models
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
    {
        if seen.insert(model.to_string()) {
            ollama_services.push(ServiceConfig::ollama(model));
        }
    }
    services.splice(insert_at..insert_at, ollama_services);
}

/// Services shown by default: one local entry, Google, and one CLI entry.
/// Every user-added model becomes another entry; nothing is hard-coded per vendor.
pub fn default_services() -> Vec<ServiceConfig> {
    vec![
        ServiceConfig::local("local-1", "本地开源模型"),
        ServiceConfig {
            id: "google".into(),
            name: "谷歌翻译".into(),
            kind: ServiceKind::Google,
            endpoint: String::new(),
            model: String::new(),
            api_key: String::new(),
        },
        ServiceConfig::openai_cli("openai-cli-1", "OpenAI（本地 CLI）", ""),
    ]
}
