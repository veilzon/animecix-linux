#!/usr/bin/env bash
# AppImage için minimal ffmpeg + libmpv derler.
# Amaç: sistemdeki tam özellikli libmpv'nin OCR/konuşma/encoder bağımlılıklarını
# (ICU, tesseract, flite, lpcnet, rav1e, x265...) AppImage'a taşımamak.
#
# Çıktı: target/minimal-libmpv/usr/lib/libmpv.so.2 ve yalnız gerekli .so'lar.
# Build bağımlılıkları CI'da kurulur; bu betik sistem paketi kurmaz.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${MINIMAL_LIBMPV_DIR:-$ROOT/target/minimal-libmpv}"
PREFIX="$DEST/usr"
SRC="$DEST/src"
JOBS="${JOBS:-$(nproc)}"
FFMPEG_VER="${FFMPEG_VER:-6.1.1}"
MPV_VER="${MPV_VER:-0.36.0}"

if [ -f "$PREFIX/lib/libmpv.so.2" ] && [ "${MINIMAL_MPV_FORCE:-0}" != "1" ]; then
    echo "==> minimal libmpv hazır: $PREFIX/lib/libmpv.so.2"
    exit 0
fi

for tool in curl tar make gcc meson ninja pkg-config; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "eksik araç: $tool" >&2
        exit 1
    }
done

mkdir -p "$SRC" "$PREFIX"
rm -rf "$DEST/build-ffmpeg" "$DEST/build-mpv"

echo "==> ffmpeg $FFMPEG_VER indiriliyor"
if [ ! -f "$SRC/ffmpeg-$FFMPEG_VER.tar.xz" ]; then
    curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VER.tar.xz" \
        -o "$SRC/ffmpeg-$FFMPEG_VER.tar.xz"
fi
if [ ! -d "$SRC/ffmpeg-$FFMPEG_VER" ]; then
    tar -xf "$SRC/ffmpeg-$FFMPEG_VER.tar.xz" -C "$SRC"
fi

echo "==> ffmpeg minimal derleniyor"
pushd "$SRC/ffmpeg-$FFMPEG_VER" >/dev/null
./configure \
    --prefix="$PREFIX" \
    --libdir="$PREFIX/lib" \
    --enable-shared --disable-static \
    --disable-programs --disable-doc --disable-debug --disable-avdevice \
    --disable-postproc --disable-everything \
    --enable-protocol=file,http,https,tcp,tls,crypto \
    --enable-demuxer=mov,matroska,mpegts,hls,dash,flv,avi \
    --enable-decoder=h264,hevc,vp9,libdav1d,aac,opus,vorbis,mp3,flac,ass,subrip,webvtt,movtext,pgssub \
    --enable-encoder=png,mjpeg \
    --enable-muxer=image2 \
    --enable-parser=h264,hevc,vp9,av1,aac,opus,vorbis,flac \
    --enable-bsf=h264_mp4toannexb,hevc_mp4toannexb \
    --enable-filter=aresample,scale,format \
    --enable-swscale --enable-swresample \
    --enable-libdav1d --enable-gnutls \
    --enable-zlib
make -j"$JOBS"
make install
popd >/dev/null

echo "==> mpv $MPV_VER indiriliyor"
if [ ! -f "$SRC/mpv-$MPV_VER.tar.gz" ]; then
    curl -fsSL "https://github.com/mpv-player/mpv/archive/refs/tags/v$MPV_VER.tar.gz" \
        -o "$SRC/mpv-$MPV_VER.tar.gz"
fi
if [ ! -d "$SRC/mpv-$MPV_VER" ]; then
    tar -xf "$SRC/mpv-$MPV_VER.tar.gz" -C "$SRC"
fi

echo "==> libmpv minimal derleniyor"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
meson setup "$DEST/build-mpv" "$SRC/mpv-$MPV_VER" \
    --prefix="$PREFIX" \
    --libdir=lib \
    --buildtype=release \
    -Db_ndebug=if-release \
    -Dbuild-date=false \
    -Dcplayer=false \
    -Dlibmpv=true \
    -Dgpl=false \
    -Dlua=disabled \
    -Djavascript=disabled \
    -Dlibarchive=disabled \
    -Dlibavdevice=disabled \
    -Dlibbluray=disabled \
    -Ddvdnav=disabled \
    -Dcdda=disabled \
    -Ddvbin=disabled \
    -Drubberband=disabled \
    -Djpeg=disabled \
    -Dlcms2=disabled \
    -Dzlib=enabled \
    -Diconv=enabled \
    -Dvulkan=disabled \
    -Dlibplacebo=disabled \
    -Dlibplacebo-next=disabled \
    -Dzimg=disabled \
    -Duchardet=disabled \
    -Dvapoursynth=disabled \
    -Dalsa=disabled \
    -Djack=disabled \
    -Dopenal=disabled \
    -Dsdl2-audio=disabled \
    -Dsdl2-video=disabled \
    -Dmanpage-build=disabled \
    -Dhtml-build=disabled \
    -Dpdf-build=disabled \
    -Dtests=false \
    -Dx11=disabled \
    -Dwayland=disabled \
    -Ddrm=disabled \
    -Dgbm=disabled \
    -Degl=disabled \
    -Degl-x11=disabled \
    -Degl-wayland=disabled \
    -Degl-drm=disabled \
    -Dgl=enabled \
    -Dplain-gl=enabled \
    -Dvaapi=disabled \
    -Dvdpau=disabled \
    -Dshaderc=disabled \
    -Dspirv-cross=disabled \
    -Dcaca=disabled \
    -Dpipewire=disabled \
    -Dpulse=enabled
ninja -C "$DEST/build-mpv"
ninja -C "$DEST/build-mpv" install

LIBMPV="$PREFIX/lib/libmpv.so.2"
if [ ! -e "$LIBMPV" ]; then
    echo "libmpv.so.2 üretilmedi" >&2
    ls -la "$PREFIX/lib" >&2 || true
    exit 1
fi

# Boyut ve ldd'nin build dizinini görmemesi için debug sembollerini at.
find "$PREFIX/lib" -maxdepth 1 -type f -name '*.so*' -exec strip --strip-unneeded {} + 2>/dev/null || true

echo "==> minimal libmpv hazır: $LIBMPV"
ls -lh "$PREFIX/lib" | head -n 20
