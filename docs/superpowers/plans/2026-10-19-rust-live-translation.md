# Rust Live Translation Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a runnable Rust macOS translation app with live chat-draft preview, explicit safe replacement, selection translation and screenshot OCR.

**Architecture:** Rust owns app state, AppKit UI, accessibility integration and worker orchestration. A narrow Swift executable bridges Apple Translation and Vision; no embedded browser runtime. Platform boundaries remain explicit.

**Tech Stack:** Rust 2024, objc2/AppKit, ApplicationServices, Swift system-framework bridge.

**Spec:** docs/superpowers/specs/2026-10-19-easytranslate-design.md, superseded by the user's approval of direct chat input and Rust on 2026-10-19.

## Global Constraints

- Never modify existing AGENTS.md. New setup information belongs in README.md.
- User approved implementation; execute locally without another design approval cycle.
- Only read focused editable controls in explicitly enabled apps; never log private text.
- Replace only after explicit action and a fresh exact target/text match; never send a message.
- No public translation endpoint dependency, automatic network fallback, or API secrets.
- First native bridge targets macOS 26+ for installed-language TranslationSession; document this changed floor.
- Preserve input if permission, focus, editability or AX replacement cannot be verified.

## Review Focus

- Async response arrives after the user types a negation: invalidate old translation.
- Identical text in a different conversation: require the same AX element and focused window.
- Translation or OCR failure: disable replacement and preserve source.
- Missing language packs: report setup requirements without fake translations.
- Unicode and multi-paragraph inputs: preserve text and whitespace, avoid byte-index truncation.

## Tasks

### Task 1: Draft state and quality fixtures
Files: src/draft.rs, tests/draft.rs, docs/quality/chat-cases.json.
Interface: Draft::observe(Option<Snapshot>), accept(generation, text), replacement(live).
- [ ] Write tests for stale results, focus changes, language changes, blank results and unchanged polls.
- [ ] Run cargo test --test draft and observe assertion failures.
- [ ] Implement generation-based invalidation and exact-match replacement guard.
- [ ] Run cargo test; prepare human-reviewed quality criteria rather than claim automatic semantic accuracy.

### Task 2: Native platform and bridge
Files: src/platform/{mod.rs,accessibility.rs}, native/TranslationBridge.swift.
Interfaces: focused_snapshot() -> Result<Focus, String>, replace(Focus,text) -> Result; JSON stdin/stdout bridge for languages/translate/OCR.
- [ ] Implement focused-editable-only AX reading, app allowlist and exact reread before replacement.
- [ ] Compile the bridge, query language readiness, exercise missing model errors with public test strings only.
- [ ] Add deterministic contract tests for request serialization and subprocess errors.

### Task 3: Window, actions and asynchronous translation
Files: src/main.rs, src/platform/ui.rs, src/bridge.rs.
- [ ] Add nonactivating floating preview, language picker, live toggle, replace/copy actions, manual reading editor.
- [ ] Debounce draft changes, discard stale completions, avoid overlapping translation processes.
- [ ] Add configurable keyboard shortcuts for toggle/manual, selection and screenshot; screen capture cancellation leaves input untouched.
- [ ] Build and smoke-test startup; record cross-app and permissions checks that require the user's interaction.

### Task 4: Packaging and verification
Files: scripts/build-app.sh, resources/Info.plist, README.md.
- [ ] Build local .app with bridge and ad-hoc signature; verify executable dependencies and bundle size.
- [ ] Run cargo test, cargo fmt --check and cargo clippy --all-targets -- -D warnings.
- [ ] Review whole change; fix critical findings; document unverified behavior without claiming production compatibility.

## Execution ledger

Ruling: The initial workspace has no Git metadata. Work in this designated empty project directory; do not create worktrees, publish or invent commits.
Ruling: User explicitly requested Rust and said to start. Implement inline without another permission request.
Ruling: Accessibility does not expose reliable marked-text state for every external app. Use a settling delay, Chinese-content filter and stale-result guard; document IME behavior as requiring manual validation.


## 2026-10-19 Current implementation and verification

The earlier app allowlist design is superseded: the user explicitly binds the current focused editable field in any app with a hotkey. Reading remains scoped to that binding. Rust/AppKit app and Swift Translation/Vision bridge are implemented; current usage is in README.md. Prior unchecked steps are historical, not a claim that test-first evidence has been reconstructed.

Implemented: generation guards, native preview/editor, selection, screenshot OCR, article splitting/cancellation/cache, model setup with cancel/status, editable translation with explicit replacement, configuration UI, Option+A / Option+D defaults, conflict rollback, 译 icon, double-click packaging and ZIP. AGENTS.md remains unchanged.

Verified: 23 deterministic tests; formatting and lint; native AX read/replace/stale rejection; Carbon conflicts/rollback/reordering; settings persistence; default and custom hotkey translation/replacement; continuous edits and empty-input disablement; selection translation preserving source; two-line image OCR and translation. Full UI screenshot hotkey → fixture drag → OCR → translation passed; original text remains unchanged.

Open acceptance limits: actual WeChat/WhatsApp AX compatibility, Chinese input-method composition, multi-monitor placement, and semantic quality. A confirmed payer mistranslation prevents claiming translation accuracy is solved. No paid account or scraped cloud endpoint has been introduced.


## 2026-10-20 User-directed service change

Mandatory language-pack setup is rejected by the user. MyMemory official online API is now the default selected provider, with one-time explicit enablement because text is transmitted to the provider. Apple offline is optional and retains model setup only in that mode. No paid account or embedded/shared key; no silent fallback. New provider is not an accuracy guarantee: 12-case online benchmark includes a before-work/end-of-work error.

User's latest testing constraint is background-only. Synthetic desktop interaction is suspended. Shared worker headless tests cover actual online calls, provider cache isolation, cancellation, detection and local long paragraphs. Whole GUI/WeChat/WhatsApp acceptance is still pending; goal is not complete.
