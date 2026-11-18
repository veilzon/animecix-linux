#!/bin/sh
# AnimeciX kaldırılırken yalnız kendi yazdığı yönetilen autostart girdisini
# siler. Kullanıcı verisine (follows.json / ledger.json) dokunmaz.
# Hem deb (DEBIAN/postrm) hem rpm (%postun) tarafından çalıştırılır.
# Yükseltmede ($1=upgrade veya 1) hiçbir şey yapmaz; girdi korunur.
case "$1" in
  remove|purge|0) ;;
  *) exit 0 ;;
esac

for d in /home/*; do
  [ -d "$d" ] || continue
  f="$d/.config/autostart/tr.com.animecix.calendar.desktop"
  if [ -f "$f" ] && grep -q '^X-AnimeciX-Managed=1' "$f" 2>/dev/null; then
    rm -f "$f" 2>/dev/null || true
  fi
done
exit 0
