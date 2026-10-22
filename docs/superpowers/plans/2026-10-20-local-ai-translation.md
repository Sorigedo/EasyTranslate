# Lightweight Local Translation Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` and `superpowers:test-driven-development`; complete each task with its stated verification.

**Goal:** Replace the unfinished 8B chat-model path with fast offline-after-setup translation using a compact translation-specific engine.

**Architecture:** Use the Rust-native Firefox Translations engine (`fxtranslate` 0.5.0). Keep route engines resident, fetch only the selected language models on demand into Application Support, and reuse the existing Rust translation pipeline for typed text, selection, OCR, and articles. Local text never leaves the Mac.

**Tech Stack:** Rust 2024, `fxtranslate` 0.5.0, serde/serde_json, AppKit, existing Swift Vision/online bridge.

**Spec:** `docs/superpowers/specs/2026-10-20-local-ai-tone-glossary-design.md`

## Global Constraints

- No Qwen, llama.cpp, chat prompts, or automatic remote text-translation fallback.
- New installs default to local translation; legacy explicit providers stay selected.
- Only model discovery and model weights may use the network; verify model hashes and preserve original text on error.
- Models are fetched only for a selected language route and cached for offline reuse.
- Preserve generation cancellation, text whitespace, selection/OCR/article flows, and user-editable output.
- Keep `AGENTS.md` untouched. The workspace has no Git metadata; do not create commits or a Git repository.

## Review Focus

- Unsupported route or failed CDN: show a clear error, keep the draft, and do not send text elsewhere. Test Tasks 1–3.
- Repeated typing while loading: reuse one resident engine and reject stale output. Test Tasks 1–2.
- Cached route while offline: translate without contacting model discovery. Test Task 1.
- Non-English pair through English pivot: retain paragraph boundaries and reject a partial result. Test Task 2.
- Old app bundle/menu action: smoke-test current status item targets and verify packaged executable timestamps. Test Tasks 3–4.

---

### Task 1: Rust translation engine and model cache

**Files:** Modify `Cargo.toml`, `Cargo.lock`, `src/local_ai.rs`, `src/lib.rs`; create `tests/local_translation.rs`.

**Interfaces:** Implement `LocalTranslator::new(cache_root) -> LocalTranslator` and `translate(text, source, target) -> Result<String, String>`. Cache one `fxtranslate::engine::Translation` per route. Use `fxtranslate::loader::load_translation` with `NetworkFetch`; use `translate_long` for sentence-safe output. Store files under `~/Library/Application Support/EasyTranslate/Models`.

- [ ] Add tests for direction-preserving language mapping, cached route reuse, offline cache use, and unsupported-route errors using an injectable model loader.
- [ ] Run `cargo test --locked --test local_translation`; confirm the new service API tests fail before implementation.
- [ ] Add pinned `fxtranslate = 0.5.0` with the `net` and `icu-segmenter` features; implement the resident route cache and disk cache.
- [ ] Run the focused tests and `cargo test --locked`.

### Task 2: Translation-service integration

**Files:** Modify `src/translation.rs`, `src/shortcuts.rs`, `src/cache.rs`, `tests/translation.rs`, `tests/shortcuts.rs`, and `tests/cache.rs`.

**Interfaces:** Make `Options: Clone` and pass `Arc<LocalTranslator>` for the local provider. Keep Google/Apple dispatch explicit. Add profile identity only for behavior that actually changes output; remove unused chat-model prompt dependencies from the local path.

- [ ] Add failing tests for local translation dispatch, both directions, article whitespace, cancellation/stale result, provider-isolated cache entries, and legacy config migration.
- [ ] Run the focused tests and verify the missing dispatch or defaults fail for the expected reason.
- [ ] Integrate the translator into the shared run path; check cancellation before and after each model call and never return partial or stale output.
- [ ] Run focused tests and the complete Rust suite.

### Task 3: Settings and menu-bar reliability

**Files:** Modify `src/platform/ui.rs`, `src/main.rs`, `tests/diagnostics.rs`, and `scripts/test-background.sh`.

- [ ] Extend smoke checks to assert every status-menu item has the expected selector, targets `Actions`, and is implemented; assert the status item is visible.
- [ ] Add regression coverage for the fresh-install local provider, provider selection, model preparation status, and offline cache diagnostics.
- [ ] Add the local translation provider to Settings and explain that each chosen route is downloaded once and then works offline; keep Google/Apple opt-in and no-fallback behavior.
- [ ] Run `cargo run --locked -- --ui-smoke`, `cargo test --locked`, `cargo fmt --check`, and clippy.

### Task 4: Accuracy gate and current app package

**Files:** Modify `README.md`, `scripts/build-app.sh`, `scripts/test-background.sh`, `docs/quality/acceptance-status.md`; create `docs/quality/local-translation-benchmark.md`.

- [ ] Add Chinese↔English customer-support cases for negation, actor, conditions, amounts, dates, model IDs, troubleshooting, paragraphs, and common IT terms.
- [ ] Run the real downloaded models, record exact route sizes, translation outputs, cold preparation time, warm latency, and memory. Reject any changed key fact; no generic accuracy claim.
- [ ] Run the full background suite, build/sign the macOS app, verify menu targets in the packaged executable, and inspect executable/bundle versions.
- [ ] Keep the model-cache path and download errors visible; record WeChat/WhatsApp foreground acceptance as unverified unless the user separately tests it.

## Self-review

- The superseded 5 GB Qwen3/llama.cpp approach is removed. The measured Chinese↔English cache totals 101.6 MiB; other route sizes must be measured before any size claim.
- Existing text-entry, selected text, OCR, article, language-swap, and provider-consent paths are covered by the shared translation service and Task 4 regression cases.
- No external text API is presented as unlimited or stable; local translation requires only one-time model setup per route.
