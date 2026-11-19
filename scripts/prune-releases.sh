#!/usr/bin/env bash
# Eski GitHub release'lerini temizler. En yeni KEEP yayınlanmış sürüm
# (ve varsa taslaklar) korunur; daha eskiler asset'leriyle silinir.
#
# Ortam değişkenleri:
#   KEEP        korunacak en yeni release sayısı (varsayılan 5)
#   DELETE_TAGS "true" ise release tag'i de silinir (varsayılan true)
#   DRY_RUN     "true" ise yalnızca listeler, silmez
set -euo pipefail

KEEP="${KEEP:-5}"
DELETE_TAGS="${DELETE_TAGS:-true}"
DRY_RUN="${DRY_RUN:-false}"

if ! [[ "$KEEP" =~ ^[0-9]+$ ]] || [ "$KEEP" -lt 1 ]; then
  echo "HATA: KEEP pozitif bir tam sayı olmalı (gelen: $KEEP)" >&2
  exit 2
fi

# createdAt<TAB>tagName; yalnız yayınlanmış (taslak olmayan) release'ler.
mapfile -t releases < <(
  gh release list --limit 100 \
    --json tagName,createdAt,isDraft \
    -q '.[] | select(.isDraft|not) | [.createdAt, .tagName] | @tsv' \
    | LC_ALL=C sort -r
)

total="${#releases[@]}"
echo "Yayınlanmış release: $total, korunacak: $KEEP (dry-run: $DRY_RUN)"

if [ "$total" -le "$KEEP" ]; then
  echo "Silinecek eski release yok."
  exit 0
fi

for ((i = KEEP; i < total; i++)); do
  line="${releases[$i]}"
  tag="${line#*$'\t'}"
  if [ -z "$tag" ]; then
    continue
  fi
  if [ "$DRY_RUN" = "true" ]; then
    echo "[dry-run] silinecek: $tag"
    continue
  fi
  args=(release delete "$tag" --yes)
  if [ "$DELETE_TAGS" = "true" ]; then
    args+=(--cleanup-tag)
  fi
  gh "${args[@]}"
  echo "silindi: $tag"
done
