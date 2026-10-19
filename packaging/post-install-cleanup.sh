#!/bin/sh
# 1.2.6/1.2.7, ikiliyi ~/.local/bin/animecix'e kopyalıyor ve masaüstü
# kısayolunu oraya yönlendiriyordu. Güncelleme sonrası bu kopya bayat kalıp
# kurulu sürüm güncel olsa da "Yeni Sürüm Mevcut" uyarısı döngüsü yaratıyordu.
# Paket kurulumunda eski kopya ve ona işaret eden kullanıcı kısayolu silinir;
# menü girdisi paketin sistem .desktop dosyasından gelir.
#
# Hem deb (DEBIAN/postinst) hem rpm (%post) tarafından çalıştırılır; root
# olarak koşar, hiçbir koşulda kurulumu düşürmemelidir.
for d in /home/*; do
  [ -d "$d" ] || continue

  f="$d/.local/bin/animecix"
  if [ -f "$f" ] && [ ! -L "$f" ] && head -c 4 "$f" 2>/dev/null | grep -q ELF; then
    rm -f "$f" 2>/dev/null || true
  fi

  for e in \
    "$d/.local/share/applications/tr.com.animecix.desktop" \
    "$d/.local/share/applications/animecix.desktop"; do
    if [ -f "$e" ] && grep -q "\.local/bin/animecix" "$e" 2>/dev/null; then
      rm -f "$e" 2>/dev/null || true
    fi
  done
done
exit 0
