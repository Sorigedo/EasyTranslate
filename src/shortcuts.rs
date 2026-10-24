use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const KEYS: &[(&str, u32)] = &[
    ("A", 0),
    ("S", 1),
    ("D", 2),
    ("F", 3),
    ("H", 4),
    ("G", 5),
    ("Z", 6),
    ("X", 7),
    ("C", 8),
    ("V", 9),
    ("B", 11),
    ("Q", 12),
    ("W", 13),
    ("E", 14),
    ("R", 15),
    ("Y", 16),
    ("T", 17),
    ("1", 18),
    ("2", 19),
    ("3", 20),
    ("4", 21),
    ("6", 22),
    ("5", 23),
    ("9", 25),
    ("7", 26),
    ("8", 28),
    ("0", 29),
    ("O", 31),
    ("U", 32),
    ("I", 34),
    ("P", 35),
    ("L", 37),
    ("J", 38),
    ("K", 40),
    ("N", 45),
    ("M", 46),
    ("SPACE", 49),
    ("F1", 122),
    ("F2", 120),
    ("F3", 99),
    ("F4", 118),
    ("F5", 96),
    ("F6", 97),
    ("F7", 98),
    ("F8", 100),
    ("F9", 101),
    ("F10", 109),
    ("F11", 103),
    ("F12", 111),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub code: u32,
    pub modifiers: u32,
}
impl Shortcut {
    pub fn from_recorded_event(code: u16, flags: usize) -> Result<Self, String> {
        let modifiers = [(17, 512), (18, 4096), (19, 2048), (20, 256)]
            .into_iter()
            .filter(|(bit, _)| flags & (1 << bit) != 0)
            .map(|(_, mask)| mask)
            .sum();
        let shortcut = Self {
            code: code.into(),
            modifiers,
        };
        Self::parse(&shortcut.label())
    }
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input
            .replace('⌘', "Command+")
            .replace('⌥', "Option+")
            .replace('⌃', "Control+")
            .replace('⇧', "Shift+");
        let mut modifiers = 0;
        let mut key = None;
        for part in input.split('+').map(|s| s.trim().to_uppercase()) {
            match part.as_str() {
                "CMD" | "COMMAND" => modifiers |= 256,
                "SHIFT" => modifiers |= 512,
                "ALT" | "OPTION" => modifiers |= 2048,
                "CTRL" | "CONTROL" => modifiers |= 4096,
                s => {
                    if key.is_some() {
                        return Err("每组快捷键只能包含一个普通按键".into());
                    }
                    key = Some(
                        KEYS.iter()
                            .find(|(name, _)| *name == s)
                            .ok_or("支持 A–Z、0–9、Space、F1–F12；不支持 Fn")?
                            .1,
                    );
                }
            }
        }
        if modifiers & (256 | 2048 | 4096) == 0 {
            return Err("至少包含 Command、Option 或 Control，避免拦截正常输入".into());
        }
        Ok(Self {
            code: key.ok_or("缺少普通按键")?,
            modifiers,
        })
    }
    pub fn label(self) -> String {
        let mut label = String::new();
        for (mask, name) in [
            (4096, "Control+"),
            (2048, "Option+"),
            (512, "Shift+"),
            (256, "Command+"),
        ] {
            if self.modifiers & mask != 0 {
                label.push_str(name);
            }
        }
        label.push_str(
            KEYS.iter()
                .find(|(_, code)| *code == self.code)
                .map(|(s, _)| *s)
                .unwrap_or("Unknown"),
        );
        label
    }
}
pub fn validate(keys: &[Shortcut; 3]) -> Result<(), String> {
    for (i, key) in keys.iter().enumerate() {
        Shortcut::parse(&key.label())?;
        if key.modifiers & !(256 | 512 | 2048 | 4096) != 0 {
            return Err("不支持的修饰键".into());
        }
        if keys[..i].contains(key) {
            return Err("三个功能不能使用同一快捷键".into());
        }
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: crate::translation::Provider,
    pub online_enabled: bool,
    #[serde(default = "crate::service::default_services")]
    pub services: Vec<crate::service::ServiceConfig>,
    #[serde(default)]
    pub active_service: usize,
    #[serde(default)]
    pub comparison_services: Vec<String>,
    #[serde(default)]
    pub llm_model: String,
    pub siliconflow_endpoint: String,
    pub siliconflow_model: String,
    pub siliconflow_api_key: String,
    pub target: String,
    pub reading_target: String,
    #[serde(default = "current_shortcut_version")]
    pub shortcut_version: u8,
    pub shortcuts: [Shortcut; 3],
    #[serde(default = "default_tone")]
    pub tone: crate::local_ai::Tone,
    #[serde(default = "crate::local_ai::seeded_glossary")]
    pub glossary: Vec<crate::local_ai::GlossaryEntry>,
}

fn current_shortcut_version() -> u8 {
    3
}

fn default_tone() -> crate::local_ai::Tone {
    crate::local_ai::Tone::CustomerService
}

impl Default for Config {
    fn default() -> Self {
        let services = crate::service::default_services();
        Self {
            provider: crate::translation::Provider::LocalAi,
            online_enabled: false,
            comparison_services: crate::service::default_comparison_services(&services),
            services,
            active_service: 0,
            llm_model: String::new(),
            siliconflow_endpoint: crate::llm::SILICONFLOW_DEFAULT_ENDPOINT.into(),
            siliconflow_model: crate::llm::SILICONFLOW_DEFAULT_MODEL.into(),
            siliconflow_api_key: String::new(),
            target: "en".into(),
            reading_target: "zh-Hans".into(),
            shortcut_version: current_shortcut_version(),
            shortcuts: [
                Shortcut {
                    code: 0,
                    modifiers: 2048,
                },
                Shortcut {
                    code: 2,
                    modifiers: 2048,
                },
                Shortcut {
                    code: 1,
                    modifiers: 2304,
                },
            ],
            tone: crate::local_ai::Tone::CustomerService,
            glossary: crate::local_ai::seeded_glossary(),
        }
    }
}
impl Config {
    pub fn path() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/EasyTranslate/config.json")
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if matches!(
            value.get("provider").and_then(|v| v.as_str()),
            Some("my_memory" | "iflytek" | "youdao")
        ) {
            value["provider"] = serde_json::json!("local_ai");
            value["online_enabled"] = serde_json::json!(false);
        }
        if value.get("provider").and_then(|v| v.as_str()) == Some("apple") {
            value["provider"] = serde_json::json!("local_ai");
        }
        if matches!(
            value.get("provider").and_then(|v| v.as_str()),
            Some("deep_seek" | "deep_seek_pro")
        ) {
            value["provider"] = serde_json::json!("silicon_flow");
        }
        let mut config: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        migrate_legacy_provider(&mut config, &value);
        if config.services.is_empty() {
            config.services = crate::service::default_services();
        } else if value.get("services").is_some()
            && !config
                .services
                .iter()
                .any(|service| service.kind == crate::service::ServiceKind::LibreTranslate)
        {
            let libretranslate_index =
                crate::service::ensure_libretranslate_service(&mut config.services);
            if libretranslate_index <= config.active_service {
                config.active_service += 1;
            }
        }
        if config.services.is_empty() || config.active_service >= config.services.len() {
            config.active_service = 0;
        }
        if value.get("comparison_services").is_none() {
            config.comparison_services =
                crate::service::default_comparison_services(&config.services);
        }
        crate::service::normalize_comparison_services(
            &config.services,
            &mut config.comparison_services,
        );
        // The selected service is the source of truth. Older builds could
        // leave the legacy provider field stale after a service changed.
        config.provider = config.services[config.active_service].provider();
        if value.get("shortcuts").is_none()
            && let Some(codes) = value.get("hotkey_codes")
        {
            let codes: [u32; 3] =
                serde_json::from_value(codes.clone()).map_err(|e| e.to_string())?;
            if codes != [49, 2, 1] {
                config.shortcuts = codes.map(|code| Shortcut {
                    code,
                    modifiers: 2304,
                });
            }
        }
        let has_saved_shortcuts = value.get("shortcuts").is_some()
            || value
                .get("hotkey_codes")
                .is_some_and(|codes| codes != &serde_json::json!([49, 2, 1]));
        if value.get("shortcut_version").is_none() && has_saved_shortcuts {
            let previous = config.shortcuts;
            let old_toggle = Shortcut::parse("Option+D")?;
            let old_selection = if previous[0] == old_toggle {
                previous[1]
            } else {
                previous[0]
            };
            config.shortcuts = [old_toggle, old_selection, previous[2]];
            config.shortcut_version = 2;
        } else if value.get("shortcut_version").is_none() {
            config.shortcut_version = current_shortcut_version();
        }
        if config.shortcut_version < current_shortcut_version() {
            config.shortcuts.swap(0, 1);
            config.shortcut_version = current_shortcut_version();
        }
        validate(&config.shortcuts)?;
        Ok(config)
    }
    pub fn load_from(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(json) => Self::from_json(&json),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn load() -> Result<Self, String> {
        Self::load_from(&Self::path())
    }
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Ok(existing) = std::fs::read_to_string(path)
            && Self::from_json(&existing).is_err()
        {
            let backup = path.with_extension("json.invalid-backup");
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(backup)?;
            std::io::Write::write_all(&mut file, existing.as_bytes())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        std::fs::rename(tmp, path)
    }
}

/// Older builds stored one hard-coded provider plus silicon-flow fields. Map
/// that state onto the service list: local/google/cli defaults, plus one
/// OpenAI-compatible entry for the previous silicon-flow settings, selected
/// to match what the user was using.
fn migrate_legacy_provider(config: &mut Config, value: &serde_json::Value) {
    if !config.services.is_empty() && !value.get("services").is_none() {
        return; // already migrated
    }
    let legacy = value.get("provider").and_then(|v| v.as_str());
    let had_services = value.get("services").is_some();
    if had_services {
        return;
    }
    let mut services = crate::service::default_services();
    let legacy_llm_model = value
        .get("llm_model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(entry) = services
        .iter_mut()
        .find(|service| service.kind == crate::service::ServiceKind::OpenAiCli)
        && !legacy_llm_model.trim().is_empty()
    {
        entry.model = legacy_llm_model;
    }
    let mut active = match legacy {
        Some("local_ai" | "apple") => 0,
        Some("google") => services
            .iter()
            .position(|service| service.kind == crate::service::ServiceKind::Google)
            .unwrap_or(0),
        Some("llm") => services
            .iter()
            .position(|service| service.kind == crate::service::ServiceKind::OpenAiCli)
            .unwrap_or(0),
        Some("silicon_flow" | "deep_seek" | "deep_seek_pro") => {
            services.push(crate::service::ServiceConfig::compatible(
                "compat-1",
                "硅基流动",
                value
                    .get("siliconflow_endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or(crate::llm::SILICONFLOW_DEFAULT_ENDPOINT),
                value
                    .get("siliconflow_model")
                    .and_then(|v| v.as_str())
                    .unwrap_or(crate::llm::SILICONFLOW_DEFAULT_MODEL),
                value
                    .get("siliconflow_api_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ));
            services.len() - 1
        }
        _ => 0,
    };
    if active >= services.len() {
        active = 0;
    }
    config.services = services;
    config.active_service = active;
}
