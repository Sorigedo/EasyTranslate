#!/bin/bash
set -euo pipefail
umask 077

support_dir="${EASYTRANSLATE_SIGNING_DIR:-$HOME/Library/Application Support/EasyTranslate/Signing}"
keychain="$support_dir/LocalSigning.keychain-db"
password_file="$support_dir/LocalSigning.password"
identity_file="$support_dir/LocalSigning.identity"
mkdir -p "$support_dir"

if [[ ! -e "$keychain" && ! -e "$password_file" && ! -e "$identity_file" ]]; then
  temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/easytranslate-signing.XXXXXX")
  trap 'rm -rf "$temporary_dir"' EXIT
  password=$(openssl rand -hex 32)
  openssl req -new -x509 -newkey rsa:3072 -nodes -days 3650 \
    -keyout "$temporary_dir/local.key" -out "$temporary_dir/local.crt" \
    -subj '/CN=EasyTranslate Local Code Signing/O=EasyTranslate' \
    -addext 'keyUsage=digitalSignature' \
    -addext 'extendedKeyUsage=codeSigning' \
    -addext 'basicConstraints=CA:FALSE' >/dev/null 2>&1
  openssl pkcs12 -export -inkey "$temporary_dir/local.key" \
    -in "$temporary_dir/local.crt" -out "$temporary_dir/local.p12" \
    -passout "pass:$password"
  security create-keychain -p "$password" "$keychain"
  security unlock-keychain -p "$password" "$keychain"
  security import "$temporary_dir/local.p12" -k "$keychain" \
    -P "$password" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$password" "$keychain" >/dev/null
  fingerprint=$(openssl x509 -in "$temporary_dir/local.crt" -noout -fingerprint -sha1)
  identity=${fingerprint#*=}
  printf '%s\n' "${identity//:/}" > "$identity_file"
  printf '%s\n' "$password" > "$password_file"
fi

if [[ ! -f "$keychain" || ! -f "$password_file" || ! -f "$identity_file" ]]; then
  echo '本地签名文件不完整；已停止打包，避免生成会再次丢失辅助功能授权的应用。' >&2
  exit 1
fi

password=$(<"$password_file")
identity=$(<"$identity_file")
security unlock-keychain -p "$password" "$keychain"
if ! security find-identity -p codesigning "$keychain" | rg -Fq "$identity"; then
  echo '本地代码签名身份不可用；已停止打包。' >&2
  exit 1
fi
printf '%s\n' "$identity"
