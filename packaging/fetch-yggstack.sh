#!/usr/bin/env bash
# Uygulamayla birlikte paketlenecek userspace Yggdrasil yardımcısını indirir.
# Kullanım: packaging/fetch-yggstack.sh [çıktı-dizini]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/target/release}"
VERSION="${YGGSTACK_VERSION:-0.0.5}"
mkdir -p "$OUT_DIR"

case "$(uname -m)" in
  x86_64|amd64)
    ASSET="yggstack-linux-amd64"
    SHA256="533cca213caa459de362a500108c6697000c3b971fa1bc905d3c8ac3d42ff48a"
    ;;
  aarch64|arm64)
    ASSET="yggstack-linux-arm64"
    SHA256="f9e629c3d36d3c2a8e306cd922ba09ea007e87dda09ca265877aa0110b0d57d1"
    ;;
  *)
    echo "desteklenmeyen mimari: $(uname -m)" >&2
    exit 1
    ;;
esac

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
URL="https://github.com/DrewCyber/yggstack-ng/releases/download/v${VERSION}/${ASSET}"
echo "==> yggstack indiriliyor: $URL"
curl -fsSL "$URL" -o "$TMP"
printf '%s  %s\n' "$SHA256" "$TMP" | sha256sum -c -
install -Dm755 "$TMP" "$OUT_DIR/yggstack"
echo "==> yggstack hazır: $OUT_DIR/yggstack"
