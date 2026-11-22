#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> AnimeciX AppImage Oluşturuluyor..."

# 0. Sürüm numarası:
#    - VERSION_BUMP=patch|minor|major env ile (default: patch, +0.0.1)
#    - minor = +0.1.0, major = +1.0.0
OLD_VER=$(grep -m1 '^version =' Cargo.toml | cut -d '"' -f2)
MAJOR=$(echo "$OLD_VER" | cut -d. -f1)
MINOR=$(echo "$OLD_VER" | cut -d. -f2)
PATCH=$(echo "$OLD_VER" | cut -d. -f3)

case "${VERSION_BUMP:-patch}" in
  none)
    NEW_MAJOR=$MAJOR
    NEW_MINOR=$MINOR
    NEW_PATCH=$PATCH
    ;;
  major)
    NEW_MAJOR=$((MAJOR + 1))
    NEW_MINOR=0
    NEW_PATCH=0
    ;;
  minor)
    NEW_MAJOR=$MAJOR
    NEW_MINOR=$((MINOR + 1))
    NEW_PATCH=0
    ;;
  patch|*)
    NEW_PATCH=$((PATCH + 1))
    if [ "$NEW_PATCH" -gt 9 ]; then
        NEW_PATCH=0
        NEW_MINOR=$((MINOR + 1))
        if [ "$NEW_MINOR" -gt 9 ]; then
            NEW_MINOR=0
            NEW_MAJOR=$((MAJOR + 1))
        else
            NEW_MAJOR=$MAJOR
        fi
    else
        NEW_MAJOR=$MAJOR
        NEW_MINOR=$MINOR
    fi
    ;;
esac
NEW_VER="${NEW_MAJOR}.${NEW_MINOR}.${NEW_PATCH}"

sed -i "0,/^version = .*/s//version = \"$NEW_VER\"/" Cargo.toml
echo "==> Sürüm yükseltildi: v$OLD_VER -> v$NEW_VER"

VERSION=$(grep -m1 '^version =' Cargo.toml | cut -d '"' -f2)

# Flatpak/AppStream metainfo sürüm kaydını da aynı sürüme getir.
python3 packaging/update-metainfo.py --write

# Otomatik GitHub Release: GITHUB_TOKEN tanımlıysa AppImage'ı bir release olarak yayınlar.
publish_release() {
  local token="${GITHUB_TOKEN:-}"
  local repo="${GITHUB_REPO:-veilzon/animecix-linux}"
  local asset="AnimeciX-x86_64.AppImage"
  if [ -z "$token" ]; then
    echo "==> GITHUB_TOKEN tanımlı değil, GitHub Release oluşturulmayacak (AppImage yerelde kaldı)."
    return 0
  fi
  if [ ! -f "$asset" ]; then
    echo "==> $asset bulunamadı, release atlanıyor."
    return 0
  fi
  echo "==> GitHub Release oluşturuluyor (v$VERSION)..."
  local resp
  resp=$(curl -sS -X POST \
    -H "Authorization: Bearer $token" \
    -H "Accept: application/vnd.github+json" \
    -H "Content-Type: application/json" \
    -d "{\"tag_name\":\"v$VERSION\",\"name\":\"AnimeciX v$VERSION\",\"body\":\"Otomatik derleme.\\n\\nAppImage tek dosyadır; uygulama başlatmada yeni sürümü kontrol eder ve kendini günceller.\"}" \
    "https://api.github.com/repos/$repo/releases")
  local upload_url
  upload_url=$(printf '%s' "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('upload_url',''))" 2>/dev/null)
  if [ -z "$upload_url" ]; then
    echo "==> Release oluşturulamadı (zaten var olabilir veya token yetersiz). Asset yüklenmedi."
    return 0
  fi
  upload_url="${upload_url%\{*}"
  echo "==> Asset yükleniyor: $asset"
  curl -sS -X POST \
    -H "Authorization: Bearer $token" \
    -H "Accept: application/vnd.github+json" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@$asset" \
    "$upload_url?name=$asset&label=$asset" >/dev/null
  echo "==> Release yayınlandı: https://github.com/$repo/releases/tag/v$VERSION"
}

# 1. Release derlemesi
echo "==> cargo build --release çalıştırılıyor (v$VERSION)..."
cargo build --release

# 2. appimagetool kontrolü ve indirme
APPIMAGETOOL=""
if command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="appimagetool"
else
    # Bozuk/önceki 302 html indirmesini temizle
    if [ -f "./appimagetool-x86_64.AppImage" ] && [ "$(wc -c < ./appimagetool-x86_64.AppImage)" -lt 100000 ]; then
        rm -f ./appimagetool-x86_64.AppImage
    fi

    if [ ! -f "./appimagetool-x86_64.AppImage" ]; then
        echo "==> appimagetool indiriliyor..."
        curl -sSLO https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x appimagetool-x86_64.AppImage
    fi
    APPIMAGETOOL="./appimagetool-x86_64.AppImage"
fi

export APPIMAGE_EXTRACT_AND_RUN=1

# 3. AppDir dizin yapısını temizle ve oluştur
APPDIR="AnimeciX.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/animecix/assets"

# 4. Binary ve ikon/asset'leri kopyala
cp target/release/animecix "$APPDIR/usr/bin/animecix"
cp target/release/animecix-calendar "$APPDIR/usr/bin/animecix-calendar"
if [ -f target/release/yggstack ]; then
    cp target/release/yggstack "$APPDIR/usr/bin/yggstack"
    chmod +x "$APPDIR/usr/bin/yggstack"
fi
if [ -d "assets" ]; then
    cp -r assets/* "$APPDIR/usr/share/animecix/assets/"
fi

ICON_SRC="assets/hicolor/256x256/apps/tr.com.animecix.png"
if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/256x256/apps/tr.com.animecix.png"
    cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/256x256/apps/animecix.png"
    cp "$ICON_SRC" "$APPDIR/tr.com.animecix.png"
    cp "$ICON_SRC" "$APPDIR/animecix.png"
    cp "$ICON_SRC" "$APPDIR/.DirIcon"
fi

# 4a. libmpv (gömülü oynatıcı) + bağımlılıkları.
# Loader çalışırken APPDIR/usr/lib/libmpv.so.2 arar; global LD_LIBRARY_PATH
# verilmez. libmpv ve yanına kopyalanan tüm .so dosyaları $ORIGIN'e bakar.
LIBMPV_SRC="${ANIMECIX_LIBMPV_PATH:-}"
MINIMAL_LIBMPV_DIR="${MINIMAL_LIBMPV_DIR:-}"
if [ -n "$MINIMAL_LIBMPV_DIR" ] && [ ! -e "$MINIMAL_LIBMPV_DIR/usr/lib/libmpv.so.2" ]; then
    echo "==> HATA: minimal libmpv yok: $MINIMAL_LIBMPV_DIR/usr/lib/libmpv.so.2"
    exit 1
fi
if [ -z "$LIBMPV_SRC" ] && [ -n "$MINIMAL_LIBMPV_DIR" ] && [ -e "$MINIMAL_LIBMPV_DIR/usr/lib/libmpv.so.2" ]; then
    LIBMPV_SRC="$MINIMAL_LIBMPV_DIR/usr/lib/libmpv.so.2"
fi
if [ -n "$LIBMPV_SRC" ] && [ -e "$LIBMPV_SRC" ]; then
    LIBMPV_SRC="$(readlink -f "$LIBMPV_SRC")"
else
    LIBMPV_SRC="$(ldconfig -p 2>/dev/null | awk '/libmpv\.so\.2 /{print $NF; exit}')"
fi
if [ -z "$LIBMPV_SRC" ] || [ ! -f "$LIBMPV_SRC" ]; then
    LIBMPV_SRC="$(find /usr/lib /usr/lib64 /usr/local/lib /app/lib -maxdepth 3 -name 'libmpv.so.2*' -print 2>/dev/null | sort | head -n1)"
fi
if [ -z "$LIBMPV_SRC" ] || [ ! -f "$LIBMPV_SRC" ]; then
    echo "==> HATA: libmpv.so.2 bulunamadı. 'mpv' veya 'libmpv-dev' kurun."
    exit 1
fi
if ! command -v patchelf >/dev/null 2>&1; then
    echo "==> HATA: patchelf gerekli (paket: patchelf)."
    exit 1
fi
mkdir -p "$APPDIR/usr/lib"
if [ -n "$MINIMAL_LIBMPV_DIR" ] && [ -d "$MINIMAL_LIBMPV_DIR/usr/lib" ]; then
    cp -a "$MINIMAL_LIBMPV_DIR/usr/lib/"*.so* "$APPDIR/usr/lib/" 2>/dev/null || true
fi
LIBMPV_REAL="$(basename "$LIBMPV_SRC")"
cp -f "$LIBMPV_SRC" "$APPDIR/usr/lib/$LIBMPV_REAL"
ln -sf "$LIBMPV_REAL" "$APPDIR/usr/lib/libmpv.so.2"
ln -sf "$LIBMPV_REAL" "$APPDIR/usr/lib/libmpv.so"

ldd_libmpv() {
    if [ -n "$MINIMAL_LIBMPV_DIR" ]; then
        LD_LIBRARY_PATH="$MINIMAL_LIBMPV_DIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" ldd "$1" 2>/dev/null
    else
        ldd "$1" 2>/dev/null
    fi
}

copy_dep() {
    local lib="$1" base
    base="$(basename "$lib")"
    case "$base" in
        libc.so*|libm.so*|libpthread.so*|libdl.so*|librt.so*|libresolv.so*|ld-linux*|libnss*|libstdc++.so*|libgcc_s.so*) return ;;
    esac
    [ -e "$APPDIR/usr/lib/$base" ] || cp -L "$lib" "$APPDIR/usr/lib/$base"
}
for lib in $(ldd_libmpv "$LIBMPV_SRC" | awk '/=> \//{print $3}'); do
    copy_dep "$lib"
done
# Kopyalanan her kütüphanenin kendi bağımlılıklarını da tara.
for copied in "$APPDIR"/usr/lib/*.so*; do
    [ -f "$copied" ] || continue
    while IFS= read -r lib; do
        copy_dep "$lib"
    done < <(ldd_libmpv "$copied" | awk '/=> \//{print $3}')
done
while IFS= read -r -d '' so; do
    patchelf --set-rpath '$ORIGIN' "$so"
done < <(find "$APPDIR/usr/lib" -maxdepth 1 -type f -name '*.so*' -print0)
echo "==> libmpv paketlendi: $(basename "$LIBMPV_SRC")"

# 4b. Adwaita symbolic ikonları standart hicolor konumuna kopyala
# (XDG_DATA_DIRS üzerinden bulunur; CC-BY-SA atıf dosyasıyla birlikte).
if [ -d "assets/hicolor/scalable/actions" ]; then
    mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/actions"
    cp assets/hicolor/scalable/actions/*.svg "$APPDIR/usr/share/icons/hicolor/scalable/actions/"
    mkdir -p "$APPDIR/usr/share/doc/animecix"
    cp assets/ATTRIBUTION-Adwaita.md "$APPDIR/usr/share/doc/animecix/" 2>/dev/null || true
    if [ -f "assets/hicolor/index.theme" ]; then
        cp assets/hicolor/index.theme "$APPDIR/usr/share/icons/hicolor/index.theme"
    else
        # Tema dosyası yoksa GTK klasörü tema saymazdı; sessiz geçme, üret.
        cat > "$APPDIR/usr/share/icons/hicolor/index.theme" <<'THEME'
[Icon Theme]
Name=hicolor
Comment=Fallback icon theme
Directories=256x256/apps,512x512/apps,scalable/actions
Version=1.0

[256x256/apps]
Size=256
Context=Applications
Type=Fixed

[512x512/apps]
Size=512
Context=Applications
Type=Fixed

[scalable/actions]
Size=16
Context=Actions
Type=Scalable
MinSize=8
MaxSize=512
THEME
        echo "==> UYARI: assets/hicolor/index.theme yoktu, AppDir için üretildi."
    fi
fi

# 4c. Adwaita tema görünümü: aynı GitHub SVG'ler `Adwaita/symbolic/actions`
# diziliminde de sunulur (sistem dizilimiyle aynı: Adwaita/*.svg oradadır).
# Uygulama temayı Adwaita'ya sabitler (main.rs); GTK önce aktif temayı
# aradığından breeze/Yaru altındaki sistem çizimleri ezilemez.
if [ -d "assets/hicolor/scalable/actions" ]; then
    mkdir -p "$APPDIR/usr/share/icons/Adwaita/symbolic/actions"
    cp assets/hicolor/scalable/actions/*.svg "$APPDIR/usr/share/icons/Adwaita/symbolic/actions/"
    cat > "$APPDIR/usr/share/icons/Adwaita/index.theme" <<'THEME'
[Icon Theme]
Name=Adwaita
Comment=AnimeciX gomulu Adwaita (GitHub master, CC-BY-SA; atif: usr/share/doc/animecix/)
Directories=symbolic/actions
Version=1.0

[symbolic/actions]
Size=16
Context=Actions
Type=Scalable
MinSize=8
MaxSize=512
THEME
fi

# 5. .desktop dosyası oluştur
cat > "$APPDIR/tr.com.animecix.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=AnimeciX
Comment=Anime, dizi ve film izleme istemcisi
Exec=animecix
Icon=tr.com.animecix
Terminal=false
Categories=AudioVideo;Video;Network;
X-AppImage-Version=$VERSION
EOF
# NOT: Tek .desktop dosyası (katalog testi "multiple desktop files" diye patlar).

# 5b. aria2c + kütüphaneleri (6 bağlantılı indirme; glibc çekirdeği hariç).
# CI'da ANIMECIX_MINIMAL_ARIA2=1 ile yalnızca HTTP/HTTPS özellikli minimal
# aria2 derlenir; BitTorrent/Metalink/XML-RPC/SFTP/ICU libleri pakete girmez.
ARIA2_BIN=""
if [ "${ANIMECIX_MINIMAL_ARIA2:-0}" = "1" ]; then
    bash packaging/build-minimal-aria2.sh
    ARIA2_BIN="target/aria2-min/bin/aria2c"
elif [ -x "target/aria2-min/bin/aria2c" ]; then
    ARIA2_BIN="target/aria2-min/bin/aria2c"
elif command -v aria2c >/dev/null 2>&1; then
    ARIA2_BIN="$(command -v aria2c)"
fi
if [ -n "$ARIA2_BIN" ]; then
    mkdir -p "$APPDIR/usr/lib"
    cp "$ARIA2_BIN" "$APPDIR/usr/bin/aria2c"
    chmod +x "$APPDIR/usr/bin/aria2c"
    for lib in $(ldd "$ARIA2_BIN" 2>/dev/null | awk '{print $3}' | grep '^/'); do
        base="$(basename "$lib")"
        case "$base" in
            libc.so*|libm.so*|libpthread.so*|libdl.so*|librt.so*|ld-linux*|libnss*|libresolv.so*|libstdc++.so*|libgcc_s.so*) continue ;;
        esac
        cp -n "$lib" "$APPDIR/usr/lib/" 2>/dev/null || true
    done
    echo "==> aria2c paketlendi: $(du -sh "$APPDIR/usr/bin/aria2c" | cut -f1)"
else
    echo "==> UYARI: aria2c yok, iç motorla devam (tek bağlantı)."
fi

# 6. AppRun betiği oluştur
cat > "$APPDIR/AppRun" <<'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
# LD_LIBRARY_PATH BURADA VERİLMEZ: GTK'nın başlattığı ffmpegthumbnailer gibi
# sistem araçları AppImage içindeki C++ runtime'ını yükleyip SELinux'a takılıyordu.
# Yalnızca paketlenmiş aria2c, kendi başlatıldığı yerde bu değişkeni alır.
export APPDIR="${HERE}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
# glibc malloc arena tavanı: çok thread'li ayırmalarda RAM şişmesini sınırlar
export MALLOC_ARENA_MAX=2
# Yayın takibi: autostart Exec'i AppImage'i gösterir; iç binary'ye yönlendir.
case "$1" in
  --calendar-daemon) shift; exec "${HERE}/usr/bin/animecix-calendar" --daemon "$@" ;;
  --calendar-once)   shift; exec "${HERE}/usr/bin/animecix-calendar" --once "$@" ;;
esac
exec "${HERE}/usr/bin/animecix" "$@"
EOF

chmod +x "$APPDIR/AppRun"

# 7. AppImage paketini oluştur
echo "==> AppImage paketleniyor..."
rm -f "AnimeciX-x86_64.AppImage"
ARCH=x86_64 "$APPIMAGETOOL" --no-appstream "$APPDIR" "AnimeciX-x86_64.AppImage"
chmod +x "AnimeciX-x86_64.AppImage"

publish_release

echo "==> Başarılı! AppImage oluşturuldu: AnimeciX-x86_64.AppImage"
