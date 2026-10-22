#[cfg(target_os = "macos")]
mod platform;

fn main() {
    #[cfg(target_os = "macos")]
    if std::env::args().any(|a| a == "--ui-smoke") {
        platform::ui::smoke();
        return;
    }
    #[cfg(target_os = "macos")]
    if std::env::args().any(|a| a == "--ui-online-smoke") {
        platform::ui::online_smoke();
        return;
    }
    #[cfg(target_os = "macos")]
    if std::env::args().any(|a| a == "--diagnostics") {
        println!(
            "Accessibility permission: {}",
            platform::accessibility::trusted()
        );
        let config = match easytranslate::shortcuts::Config::load() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("无法读取翻译设置：{error}");
                std::process::exit(1);
            }
        };
        println!("翻译服务：{}", config.provider.label());
        if matches!(
            config.provider,
            easytranslate::translation::Provider::SiliconFlow
                | easytranslate::translation::Provider::Llm
        ) {
            println!("在线模型诊断不会发送文字或测试网络，也不会显示密钥。");
            if config.provider == easytranslate::translation::Provider::SiliconFlow {
                println!(
                    "硅基流动 API Key：{}",
                    if !config.siliconflow_api_key.trim().is_empty() {
                        "已配置"
                    } else {
                        "未配置"
                    }
                );
                println!("硅基流动模型：{}", config.siliconflow_model);
            }
            let codex = if config.provider == easytranslate::translation::Provider::Llm {
                easytranslate::llm::LlmSettings::discover_codex_with_model(&config.llm_model)
            } else {
                easytranslate::llm::LlmSettings::discover_with_model(&config.llm_model)
            };
            match codex {
                Ok(settings) => {
                    let purpose = if config.provider == easytranslate::translation::Provider::Llm {
                        "OpenAI / 本机 Codex 可用"
                    } else {
                        "OpenAI / Codex 兜底可用"
                    };
                    println!("{purpose}，模型：{}", settings.model);
                }
                Err(error) => println!("OpenAI / Codex 配置不可用：{error}"),
            }
            println!(
                "联网翻译：{}",
                if config.online_enabled {
                    "已启用"
                } else {
                    "未启用"
                }
            );
            return;
        }
        if config.provider == easytranslate::translation::Provider::LocalAi {
            let root = easytranslate::local_ai::LocalTranslator::default_cache_root();
            let cache = fxtranslate::cache::Cache::with_root(root);
            println!("本地模型缓存：{}", cache.root().display());
            match cache.list_cached() {
                Ok(pairs) if pairs.is_empty() => {
                    println!("尚未下载模型；正常启动时会在后台预热默认语言方向。")
                }
                Ok(pairs) => {
                    for pair in pairs {
                        println!(
                            "已缓存 {} ({:.1} MiB)",
                            pair.name,
                            pair.bytes as f64 / 1_048_576.0
                        );
                    }
                }
                Err(error) => {
                    eprintln!("读取本地模型缓存失败：{error}");
                    std::process::exit(1);
                }
            }
            println!(
                "模型由 Mozilla Remote Settings/CDN 提供；缓存后支持离线翻译。原文不会发往模型服务。"
            );
        }
        return;
    }
    #[cfg(target_os = "macos")]
    platform::ui::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("EasyTranslate currently requires macOS 26 or later.");
}
