# Lightweight Local Translation Design

Date: 2026-10-20. Status: approved by the user; model direction updated after review.

## Intent

Provide fast, accurate, free, offline-after-setup translation in the existing Rust menu-bar application. Use a purpose-built machine-translation model rather than a general chat/LLM model. Keep the user's original text and translation on the Mac.

## Confirmed decisions

- Use the Firefox Translations Marian engine through `fxtranslate` 0.5.0, a Rust-native, CPU-only implementation. Do not bundle `llama.cpp`, Qwen, or another chat model.
- Keep Chinese↔English as the first verified pair. Other supported target languages may prepare small direction-specific models on first use; verified models remain cached and work offline. Non-English-to-non-English translation may use two English-pivot models.
- On this Apple Silicon machine, the two Chinese↔English direction caches were downloaded and verified at 101.6 MiB total. Warm one-sentence runs took about 0.2 seconds of translator time in the CLI. These are observed values, not guarantees for every pair or machine.
- Model files are fetched from Mozilla Remote Settings/CDN and hash-verified by the engine. Translation text is never sent to Mozilla or another provider. Initial model setup needs network access; a cached pair works offline. If model discovery/download fails, show a clear error and never fall back to a remote text-translation service.
- Keep Google and Apple as explicit user-selected alternatives. New installations default to local translation; legacy explicit provider selections remain unchanged.
- Preserve the existing input popup, selection translation, screenshot/OCR translation, article handling, language swap, cancellation, cache isolation, and editable result. Do not read chat input fields, auto-replace text in other applications, or send messages.
- Translate the source faithfully, including politeness and register; do not add apologies, promises, facts, or a generated rewrite. The Marian engine does not accept free-form tone prompts, so the UI must not imply otherwise. Check IT support terms in the fixed quality corpus before release.
- Keep `AGENTS.md` unchanged.

## Implementation notes

Use one resident `fxtranslate::engine::Translation` per language route so typing does not reload model weights. Store verified files under EasyTranslate Application Support. Reuse the crate's supported-version routing and SHA-256 validation. A model-load or network error must be visible while retaining the original input; a previous generation must never overwrite a newer draft.

The first model source is Mozilla's public model CDN. It is independent of translation-text APIs and has no character quota; first-time availability still depends on that CDN. Preserve exact model versions and hashes in diagnostics/acceptance records, and report model-download failures instead of claiming universal availability.

## Acceptance

- Test Chinese↔English negation, speaker/action ownership, conditions, amounts, dates, product IDs, troubleshooting steps, polite customer-support wording, line breaks, and IT terminology.
- Reject release for a changed negation, actor, amount, date, identifier, or troubleshooting action.
- Warm short-message translation target: under 3 seconds with the model resident; record cold-load time and peak memory separately.
- Test model-cache success, cache reuse while offline, unavailable CDN, corrupt model files, cancellation/stale input, and provider selection without desktop automation or real chat content.
- Verify the newly built app bundle is newer than the running installation and that every status-menu item has a live target/action. Do not claim foreground WeChat/WhatsApp acceptance unless separately observed.
