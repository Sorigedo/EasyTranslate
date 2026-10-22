#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build dist/EasyTranslate.app/Contents/MacOS dist/EasyTranslate.app/Contents/Resources
arch_name=$(uname -m)
if [[ "$arch_name" != arm64 && "$arch_name" != x86_64 ]]; then
  echo "Unsupported architecture: $arch_name" >&2; exit 1
fi
xcrun swiftc -parse-as-library -O -target "$arch_name-apple-macos26.0" native/TranslationBridge.swift -o .build/TranslationBridge
MACOSX_DEPLOYMENT_TARGET=26.0 cargo build --release --locked
cp target/release/easytranslate dist/EasyTranslate.app/Contents/MacOS/
cp .build/TranslationBridge dist/EasyTranslate.app/Contents/MacOS/
xcrun swift scripts/make-icon.swift .build/AppIcon.iconset
iconutil -c icns .build/AppIcon.iconset -o dist/EasyTranslate.app/Contents/Resources/AppIcon.icns
cp .build/MenuBarTemplate.png dist/EasyTranslate.app/Contents/Resources/
cp resources/Info.plist dist/EasyTranslate.app/Contents/Info.plist
if [[ -n "${EASYTRANSLATE_SIGN_IDENTITY:-}" ]]; then
  sign_options=(--sign "$EASYTRANSLATE_SIGN_IDENTITY")
else
  sign_identity=$(./scripts/ensure-local-signing.sh)
  sign_keychain="${EASYTRANSLATE_SIGNING_DIR:-$HOME/Library/Application Support/EasyTranslate/Signing}/LocalSigning.keychain-db"
  sign_options=(--sign "$sign_identity" --keychain "$sign_keychain")
fi
codesign --force "${sign_options[@]}" dist/EasyTranslate.app/Contents/MacOS/TranslationBridge
codesign --force "${sign_options[@]}" dist/EasyTranslate.app
codesign --verify --deep --strict dist/EasyTranslate.app
if [[ -z "${EASYTRANSLATE_SIGN_IDENTITY:-}" ]]; then
  requirement=$(codesign -dr- dist/EasyTranslate.app 2>&1)
  if [[ "$requirement" != *"certificate root"* ]]; then
    echo '签名未生成稳定的应用身份；已停止打包。' >&2
    exit 1
  fi
fi
ls -lh dist/EasyTranslate.app/Contents/MacOS/
