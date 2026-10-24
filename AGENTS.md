# Repository Guidelines

## Project Structure & Module Organization

`easytranslate` is a Rust macOS menu-bar translation app (edition 2024).

- `src/` — application and library code. Single-purpose modules (e.g. `translation.rs`, `shortcuts.rs`, `cache.rs`, `capture.rs`, `llm.rs`, `local_ai.rs`); macOS-specific code lives under `src/platform/`.
- `tests/` — integration tests, usually one file per module (e.g. `src/translation.rs` <-> `tests/translation.rs`).
- `examples/` — standalone check/benchmark binaries (`hotkey_check.rs`, `local_benchmark.rs`, ...).
- `scripts/` — build, signing, icon, and auxiliary test scripts (Swift/Python/Shell).
- `docs/`, `resources/` — documentation and assets; `dist/` and `target/` are build outputs, do not commit changes there.

## Build, Test, and Development Commands

Requires macOS 26+, Rust, and Apple Command Line Tools.

- `cargo build` — build the binary and library.
- `cargo test --locked` — run the full test suite.
- `cargo fmt --check` — verify formatting (run `cargo fmt` to fix).
- `cargo clippy --all-targets -- -D warnings` — lint; all warnings are errors.
- `bash scripts/test-background.sh` — background-safe test script; it must not activate windows, simulate input, read chat/clipboard, or download translation models.
- `./run.command` — package `dist/EasyTranslate.app` plus ZIP and DMG with local signing.

## Coding Style & Naming Conventions

- Standard `rustfmt` formatting (4-space indent); always pass `cargo fmt --check`.
- Clippy must be clean with `-D warnings`.
- Use idiomatic Rust naming: `snake_case` functions/modules, `CamelCase` types, `SCREAMING_SNAKE_CASE` constants.
- Keep modules focused; prefer adding a new `src/<feature>.rs` with a matching `tests/<feature>.rs`.
- Never fake translation output or silently switch services on failure — surface errors explicitly.

## Testing Guidelines

Tests are Rust integration tests run via `cargo test --locked`; name files after the module under test. Auxiliary scripts (`scripts/test-ocr.py`, `scripts/test-language-detection.py`, `scripts/test-workflow.py`, `scripts/test-ax.sh`) cover OCR, language detection, and accessibility workflows. New features should include tests for cancellation, stale results, caching, and shortcut behavior where applicable. Run the full suite plus clippy before submitting.

## Commit & Pull Request Guidelines

No strict Git history convention is enforced; use short, imperative commit subjects (e.g. "Fix stale-result handling in translation window") and keep commits scoped to one change.

Pull requests should include:

- A clear description of the change and motivation.
- Linked issue (if any) and test evidence (`cargo test --locked` output).
- Screenshots or screen recordings for UI/window-placement changes.
- A note on any privacy-relevant behavior (network access, clipboard, permissions).

## Security & Configuration Tips

Online translation (SiliconFlow / OpenAI / Codex) requires explicit user authorization; API keys live in `~/Library/Application Support/EasyTranslate/config.json` and must stay user-readable only. Never hardcode keys, log secrets, or add network calls that bypass the authorization gate.
