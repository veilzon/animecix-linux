#!/usr/bin/env bash
# AppImage için minimal aria2c: yalnızca HTTP/HTTPS indirme.
# BitTorrent, Metalink, XML-RPC, SFTP, c-ares ve ICU/libxml2 dışarıda kalır.
set -euo pipefail

VERSION="1.37.0"
SHA256="60a420ad7085eb616cb6e2bdf0a7206d68ff3d37fb5a956dc44242eb2f79b66b"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/target/aria2-min"
SRC="$ROOT/target/aria2-src"
TARBALL="$SRC/aria2-$VERSION.tar.xz"

if [ -x "$DEST/bin/aria2c" ]; then
    echo "==> Minimal aria2 zaten hazır: $DEST/bin/aria2c"
    exit 0
fi

mkdir -p "$SRC"
if [ ! -f "$TARBALL" ]; then
    echo "==> aria2 $VERSION indiriliyor..."
    curl -fsSL \
        "https://github.com/aria2/aria2/releases/download/release-$VERSION/aria2-$VERSION.tar.xz" \
        -o "$TARBALL"
fi
echo "$SHA256  $TARBALL" | sha256sum -c -

rm -rf "$SRC/aria2-$VERSION"
tar -xJf "$TARBALL" -C "$SRC"
cd "$SRC/aria2-$VERSION"

echo "==> Minimal aria2 yapılandırılıyor..."
./configure \
    --prefix="$DEST" \
    --disable-bittorrent \
    --disable-metalink \
    --disable-websocket \
    --disable-nls \
    --without-libxml2 \
    --without-libexpat \
    --without-libssh2 \
    --without-sqlite3 \
    --without-libcares \
    --without-libuv \
    --without-openssl

echo "==> Minimal aria2 derleniyor..."
make -j"${JOBS:-$(nproc)}"
make install
if command -v strip >/dev/null 2>&1; then
    strip --strip-all "$DEST/bin/aria2c"
fi

"$DEST/bin/aria2c" --version | sed -n '/Enabled Features/p;/Libraries/p'
echo "==> Minimal aria2 hazır: $DEST/bin/aria2c"
