#!/bin/bash
# No window activation, synthetic input, chat reads, screenshots or model downloads.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build
xcrun swiftc -parse-as-library -O native/TranslationBridge.swift -o .build/TranslationBridge
cargo test --locked
cargo run --locked -- --ui-smoke
cargo fmt --check
cargo clippy --all-targets -- -D warnings
python3 scripts/test-language-detection.py --bridge .build/TranslationBridge
if [[ "${1:-}" == "--online" ]]; then
  cargo run --locked -- --ui-online-smoke
fi
