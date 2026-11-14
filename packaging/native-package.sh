#!/usr/bin/env bash
# AnimeciX native paket entegrasyonu.
# GitHub'dan indirilen deb/rpm paketlerini kurar; sistem paketini kaldırır;
# eski AppImage kullanıcı kısayollarını temizler. Sabit AppImage kopyasını silmez.
set -eu

DRY_RUN=0
ARGS=()
for arg in "$@"; do
    if [ "$arg" = "--dry-run" ]; then
        DRY_RUN=1
    else
        ARGS+=("$arg")
    fi
done
set -- "${ARGS[@]}"

die() { printf 'native-package: %s\n' "$*" >&2; exit 1; }

owner() {
    if [ -n "${APPIMAGE:-}" ]; then
        echo appimage
        return
    fi
    if command -v dpkg-query >/dev/null 2>&1 \
        && dpkg-query -W -f='${Status}' animecix 2>/dev/null | grep -q 'install ok installed'; then
        echo deb
        return
    fi
    if command -v rpm >/dev/null 2>&1 && rpm -q animecix >/dev/null 2>&1; then
        echo rpm
        return
    fi
    echo source
}

manager() {
    if command -v apt-get >/dev/null 2>&1; then
        echo apt-get
    elif command -v dnf >/dev/null 2>&1; then
        echo dnf
    elif command -v zypper >/dev/null 2>&1; then
        echo zypper
    elif command -v pacman >/dev/null 2>&1; then
        echo pacman
    else
        die "desteklenen paket yöneticisi bulunamadı"
    fi
}

run_priv() {
    if [ "$DRY_RUN" = 1 ]; then
        if [ "$(id -u)" -eq 0 ]; then
            printf '%q ' "$@"
        else
            printf '%q ' pkexec "$@"
        fi
        printf '\n'
        return 0
    fi
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    else
        command -v pkexec >/dev/null 2>&1 || die "pkexec bulunamadı"
        pkexec "$@"
    fi
}

install_pkg() {
    [ "$#" -ge 1 ] || die "install: paket yolu gerekli"
    local pkg="$1"
    [ -f "$pkg" ] || die "paket bulunamadı: $pkg"
    local m
    m=$(manager)
    case "$m" in
        apt-get) run_priv apt-get install -y -o DPkg::Lock::Timeout=120 "$pkg" ;;
        dnf) run_priv dnf install -y "$pkg" ;;
        zypper) run_priv zypper --non-interactive install --allow-unsigned-rpm "$pkg" ;;
        pacman) run_priv pacman -U --noconfirm "$pkg" ;;
        *) die "bilinmeyen paket yöneticisi: $m" ;;
    esac
}

uninstall_native() {
    local m
    m=$(manager)
    case "$m" in
        apt-get) run_priv apt-get remove -y animecix ;;
        dnf) run_priv dnf remove -y animecix ;;
        zypper) run_priv zypper --non-interactive remove animecix ;;
        pacman) run_priv pacman -Rns --noconfirm animecix ;;
        *) die "bilinmeyen paket yöneticisi: $m" ;;
    esac
}

remove_file() {
    if [ "$DRY_RUN" = 1 ]; then
        printf 'rm -f %q\n' "$1"
    else
        rm -f "$1" 2>/dev/null || true
    fi
}

uninstall_user() {
    [ -n "${HOME:-}" ] || die "HOME yok"
    remove_file "$HOME/.local/bin/animecix"
    remove_file "$HOME/.local/bin/AnimeciX-x86_64.AppImage"
    for n in tr.com.animecix.desktop animecix.desktop; do
        remove_file "$HOME/.local/share/applications/$n"
    done
    for n in tr.com.animecix.png animecix.png tr.com.animecix.svg animecix.svg; do
        remove_file "$HOME/.local/share/icons/$n"
    done
    for n in tr.com.animecix.png animecix.png; do
        remove_file "$HOME/.local/share/icons/hicolor/256x256/apps/$n"
        remove_file "$HOME/.local/share/icons/hicolor/512x512/apps/$n"
    done
    if [ "$DRY_RUN" = 0 ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
    fi
}

desktop_exec() {
    [ -f "$1" ] || return 1
    sed -n 's/^Exec=//p' "$1" | head -n 1 | tr -d '"'
}

cleanup_home() {
    local home="$1"
    [ -n "$home" ] || return 0
    local apps="$home/.local/share/applications"
    for n in tr.com.animecix.desktop animecix.desktop; do
        local f="$apps/$n"
        [ -f "$f" ] || continue
        local ex
        ex=$(desktop_exec "$f" || true)
        case "$ex" in
            *AnimeciX-x86_64.AppImage*|*/.local/bin/animecix|/usr/bin/animecix)
                remove_file "$f"
                ;;
        esac
    done
    for n in tr.com.animecix.png animecix.png tr.com.animecix.svg animecix.svg; do
        remove_file "$home/.local/share/icons/$n"
    done
    if [ "$DRY_RUN" = 0 ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps" >/dev/null 2>&1 || true
    fi
}

cleanup_user() {
    [ -n "${HOME:-}" ] || die "HOME yok"
    cleanup_home "$HOME"
}

cleanup_all_users() {
    [ "$(id -u)" -eq 0 ] || die "cleanup --all-users root gerektirir"
    for home in /home/*; do
        [ -d "$home" ] && cleanup_home "$home"
    done
}

status() {
    printf 'owner=%s\nmanager=%s\n' "$(owner)" "$(manager)"
}

cmd="${1:-}"
shift || true
case "$cmd" in
    status)
        status
        ;;
    install)
        install_pkg "$@"
        ;;
    uninstall)
        mode="${1:-}"
        shift || true
        case "$mode" in
            --native) uninstall_native ;;
            --user) uninstall_user ;;
            *) die "uninstall: --native veya --user gerekli" ;;
        esac
        ;;
    cleanup)
        mode="${1:-}"
        shift || true
        case "$mode" in
            --user) cleanup_user ;;
            --all-users) cleanup_all_users ;;
            *) die "cleanup: --user veya --all-users gerekli" ;;
        esac
        ;;
    *)
        die "kullanım: native-package.sh [--dry-run] status|install <pkg>|uninstall --native|--user|cleanup --user|--all-users"
        ;;
esac
