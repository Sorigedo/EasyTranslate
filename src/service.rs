use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    LocalAi,
    Ollama,
    Google,
    LibreTranslate,
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
            ServiceKind::LibreTranslate => crate::translation::Provider::LibreTranslate,
            ServiceKind::OpenAiCli => crate::translation::Provider::Llm,
            ServiceKind::OpenAiCompatible => crate::translation::Provider::SiliconFlow,
        }
    }

    pub fn model_for_translation<'a>(&'a self, codex_model: &'a str) -> &'a str {
        match self.kind {
            ServiceKind::Ollama => self.model.trim(),
            ServiceKind::LibreTranslate => self.endpoint.trim(),
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

    pub fn libretranslate() -> Self {
        Self {
            id: "libretranslate".into(),
            name: "LibreTranslate（开源 API）".into(),
            kind: ServiceKind::LibreTranslate,
            endpoint: String::new(),
            model: String::new(),
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

/// Adds the built-in open-source translation service to configurations saved by
/// versions that only offered Google and local translation.
pub fn ensure_libretranslate_service(services: &mut Vec<ServiceConfig>) -> usize {
    if let Some(index) = services
        .iter()
        .position(|service| service.kind == ServiceKind::LibreTranslate)
    {
        return index;
    }
    let insert_at = services
        .iter()
        .position(|service| service.kind == ServiceKind::OpenAiCli)
        .unwrap_or(services.len());
    services.insert(insert_at, ServiceConfig::libretranslate());
    insert_at
}

/// Services shown by default: local, Google, LibreTranslate, and CLI entries.
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
        ServiceConfig::libretranslate(),
        ServiceConfig::openai_cli("openai-cli-1", "OpenAI（本地 CLI）", ""),
    ]
}

/// Returns the three comparison services shown on the translation panel by default.
/// Prefer an installed Ollama model when one exists; otherwise keep the built-in
/// OpenAI CLI service as the third selectable entry.
pub fn default_comparison_services(services: &[ServiceConfig]) -> Vec<String> {
    let google = services
        .iter()
        .find(|service| service.kind == ServiceKind::Google)
        .map(|service| service.id.clone());
    let local = services
        .iter()
        .find(|service| service.kind == ServiceKind::LocalAi)
        .map(|service| service.id.clone());
    let libretranslate = services
        .iter()
        .find(|service| service.kind == ServiceKind::LibreTranslate)
        .map(|service| service.id.clone());
    let ollama = services
        .iter()
        .filter(|service| service.kind == ServiceKind::Ollama)
        .find(|service| service.model.to_lowercase().contains("12b"))
        .or_else(|| {
            services
                .iter()
                .find(|service| service.kind == ServiceKind::Ollama)
        })
        .map(|service| service.id.clone());
    let fallback = services
        .iter()
        .find(|service| service.kind == ServiceKind::OpenAiCli)
        .or_else(|| services.first())
        .map(|service| service.id.clone());
    [google.or(libretranslate), local, ollama.or(fallback)]
        .into_iter()
        .flatten()
        .collect()
}

/// Keeps persisted comparison selections valid after services are added or removed.
pub fn normalize_comparison_services(services: &[ServiceConfig], selected: &mut Vec<String>) {
    let defaults = default_comparison_services(services);
    let mut normalized = Vec::with_capacity(3);
    let mut used = std::collections::HashSet::new();
    for candidate in selected.iter().chain(defaults.iter()) {
        if normalized.len() == 3 {
            break;
        }
        if services.iter().any(|service| service.id == *candidate) && used.insert(candidate.clone())
        {
            normalized.push(candidate.clone());
        }
    }
    for service in services {
        if normalized.len() == 3 {
            break;
        }
        if used.insert(service.id.clone()) {
            normalized.push(service.id.clone());
        }
    }
    *selected = normalized;
}

/// Returns all configured services in the order they appear in settings.
pub fn comparison_service_choices(services: &[ServiceConfig]) -> Vec<String> {
    services.iter().map(|service| service.id.clone()).collect()
}

/// Assigns a service to one comparison card. Selecting a service already
/// used by another card swaps the two assignments so every card stays unique.
pub fn assign_comparison_service(
    services: &[ServiceConfig],
    selected: &[String],
    slot: usize,
    service_id: &str,
) -> Option<Vec<String>> {
    if slot >= selected.len() || !services.iter().any(|service| service.id == service_id) {
        return None;
    }
    let mut assigned = selected.to_vec();
    let current = assigned[slot].clone();
    if current == service_id {
        return Some(assigned);
    }
    if let Some(other_slot) = assigned
        .iter()
        .enumerate()
        .find_map(|(index, selected_id)| {
            (index != slot && selected_id == service_id).then_some(index)
        })
    {
        assigned[other_slot] = current;
    }
    assigned[slot] = service_id.to_owned();
    normalize_comparison_services(services, &mut assigned);
    Some(assigned)
}
