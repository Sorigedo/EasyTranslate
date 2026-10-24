#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
trap 'code=$?; if [ "$code" -ne 0 ]; then echo "打包失败，请查看上面的错误。需要 macOS 26+、Rust 和 Apple Command Line Tools。"; fi; if [ -t 0 ]; then read -r -p "按回车关闭窗口…"; fi' EXIT
./scripts/test-background.sh
./scripts/build-app.sh
python3 scripts/test-language-detection.py --bridge dist/EasyTranslate.app/Contents/MacOS/TranslationBridge
/usr/bin/ditto -c -k --sequesterRsrc --keepParent dist/EasyTranslate.app dist/EasyTranslate.zip
/usr/bin/hdiutil create -volname EasyTranslate -srcfolder dist/EasyTranslate.app -ov -format UDZO dist/EasyTranslate.dmg
echo "打包完成：$PWD/dist/EasyTranslate.app"
echo "压缩包：$PWD/dist/EasyTranslate.zip"
echo "安装包：$PWD/dist/EasyTranslate.dmg"
if [ -t 0 ]; then open -R dist/EasyTranslate.app; fi
