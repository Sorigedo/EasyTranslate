# EasyTranslate acceptance status

Updated: 2026-10-20. Background-only validation remains the active constraint.

| Requirement | Current evidence | Remaining acceptance |
| --- | --- | --- |
| Rust lightweight desktop app | Rust/AppKit app, Swift bridge, menu-bar accessory policy, release bundle and fixed local signature | Startup and resource measurements on target machines |
| Hotkey and translation window | Carbon registration/rollback tests; menu-bar UI smoke verifies visible `译` item and menu actions | Visual check on the target display |
| Preview updates while typing | 350 ms debounce, generation cancellation, stale-output and cache tests | Chinese IME composition in WeChat and WhatsApp |
| Copy result without sending | Editable/selectable result, clear/retry states, draft guards and clipboard unit coverage | Manual paste into each client; no-send behavior |
| Custom shortcuts | Parser, duplicate detection, persistence, recording rollback and default `⌥A/⌥D/⌘⌥S` tests | Visual settings acceptance and conflict with installed apps |
| Selection, articles and screenshots | Selection flow, paragraph splitting, local OCR, cancellation and invalid-image tests | Foreground selection and multi-display screenshot checks |
| Unlimited local translation | Default `LocalAi`; no account or character quota; Mozilla model cache is local after download | First download on a clean machine |
| Translation speed and quality | `fxtranslate` benchmark: `zh-Hans→en` model 54,556,676 bytes, `en→zh-Hans` 51,934,991 bytes; cold route preparation 2.8 s / 23.8 s, warm samples 39–81 ms | Larger corpus and human review; known deadline/word-choice errors remain |
| Packaging | `打包.command` produces app, ZIP and DMG; `codesign --verify --deep --strict` passes | No notarization or universal binary requested; Intel not tested |

## Local model benchmark

`cargo run --locked --example local_benchmark` uses only public strings in `docs/quality/chat-cases.json`. Both directions translate successfully and the second run reuses cached models. The benchmark preserves numbers, names, negation and paragraph breaks in the checked samples, but it also produced wording errors such as “before work today” and “on Friday” where “before the end of work today” and “by Friday” were intended. These results support speed and operation claims, not semantic accuracy guarantees.

The local model is Marian machine translation. It does not accept free-form tone prompts; the saved tone field isolates caches for future providers, while the seeded IT glossary documents preferred terminology. Users must review payment, dates, responsibility, negation and specialist terms before sending.

## Background test boundary

`scripts/test-background.sh` runs Rust, AppKit, Swift bridge, transport, language-detection, lint and UI smoke checks without activating windows, simulating keyboard/mouse input, reading chat text or using the system clipboard. It cannot prove actual WeChat/WhatsApp focus, IME composition or replacement behavior. Those checks require a disposable unsent draft and explicit foreground testing.
