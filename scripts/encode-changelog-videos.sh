#!/usr/bin/env bash
# Sürüm-notları carousel'i için tanıtım videolarını üretir.
# Kaynaklar repo kökündedir; çıktılar assets/changelog/ altına yazılır.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/assets/changelog"
mkdir -p "$OUT"

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg gerekli." >&2
    exit 1
fi

encode() {
    local src="$ROOT/$1" dst="$OUT/$2"
    if [ ! -f "$src" ]; then
        echo "kaynak yok: $src" >&2
        exit 1
    fi
    echo "==> $1 -> assets/changelog/$2"
    nice -n 10 ffmpeg -hide_banner -loglevel error -y -i "$src" \
        -vf "scale=1080:1080:flags=lanczos" \
        -c:v libx264 -preset veryslow -crf 20 -profile:v high \
        -pix_fmt yuv420p -movflags +faststart -map_metadata -1 -an \
        "$dst"
}

encode ekrangoruntusu.mp4 ekrangoruntusu.mp4
encode fansubsecimi.mp4 fansubsecimi.mp4
encode hizsecimi.mp4 hizsecimi.mp4

echo "==> toplam: $(du -ch "$OUT"/*.mp4 | tail -n1 | cut -f1)"
