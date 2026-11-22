Name:           animecix
Version:        1.4.0
Release:        1%{?dist}
Summary:        Türkçe anime, dizi ve film izleme istemcisi
License:        MIT
URL:            https://github.com/veilzon/animecix-linux
Source0:        https://github.com/veilzon/animecix-linux/archive/refs/tags/v%{version}.tar.gz
%ifarch aarch64
Source1:        https://github.com/DrewCyber/yggstack-ng/releases/download/v0.0.5/yggstack-linux-arm64
%else
Source1:        https://github.com/DrewCyber/yggstack-ng/releases/download/v0.0.5/yggstack-linux-amd64
%endif

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
BuildRequires:  alsa-lib-devel
BuildRequires:  pkgconf-pkg-config
BuildRequires:  cmake
BuildRequires:  gcc-c++
BuildRequires:  make
BuildRequires:  clang-devel
BuildRequires:  perl
BuildRequires:  nasm
BuildRequires:  zlib-devel
BuildRequires:  git

Requires:       mpv
Requires:       mpv-libs
Requires:       alsa-lib
Requires:       libglvnd-gles
Requires:       gtk4
Requires:       libadwaita
Requires:       aria2

%description
AnimeciX; GTK4/libadwaita ile yazılmış, gömülü libmpv tabanlı Türkçe anime,
dizi ve film izleme istemcisidir. İndirme yöneticisi, Discord Rich
Presence, intro/outro atlama ve çevrimdışı oynatma içerir.

%prep
%autosetup -n animecix-linux-%{version}

%build
cargo build --release --locked

%install
install -Dm755 target/release/animecix %{buildroot}%{_bindir}/animecix
install -Dm755 target/release/animecix-calendar %{buildroot}%{_bindir}/animecix-calendar
install -Dm755 %{SOURCE1} %{buildroot}%{_bindir}/yggstack

mkdir -p %{buildroot}%{_datadir}/animecix/assets
cp -r assets/* %{buildroot}%{_datadir}/animecix/assets/
install -Dm755 packaging/post-install-cleanup.sh \
    %{buildroot}%{_datadir}/animecix/post-install-cleanup.sh
install -Dm755 packaging/post-remove-cleanup.sh \
    %{buildroot}%{_datadir}/animecix/post-remove-cleanup.sh
if [ -f assets/ATTRIBUTION-yggstack.md ]; then
    install -Dm644 assets/ATTRIBUTION-yggstack.md \
        %{buildroot}%{_datadir}/animecix/ATTRIBUTION-yggstack.md
else
    mkdir -p %{buildroot}%{_datadir}/animecix
    cat > %{buildroot}%{_datadir}/animecix/ATTRIBUTION-yggstack.md <<'EOF'
# yggstack (yggstack-ng)

- Proje: https://github.com/DrewCyber/yggstack-ng
- Sürüm: v0.0.5
- Kaynak kodu: https://github.com/DrewCyber/yggstack-ng/tree/v0.0.5
- Lisans: MPL-2.0 - https://mozilla.org/MPL/2.0/
EOF
fi

mkdir -p %{buildroot}%{_datadir}/applications
cat > %{buildroot}%{_datadir}/applications/tr.com.animecix.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=AnimeciX
GenericName=Anime İzleme Uygulaması
Comment=Anime, dizi ve film izleme istemcisi
Exec=animecix
Icon=tr.com.animecix
Terminal=false
Categories=AudioVideo;Video;
StartupWMClass=tr.com.animecix
Keywords=anime;video;player;izle;
EOF

for s in 64 128 256 512; do
    install -Dm644 "assets/icon-${s}.png" \
        "%{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps/tr.com.animecix.png"
done

%post
sh %{_datadir}/animecix/post-install-cleanup.sh || :

%preun
if [ "$1" = "0" ]; then
  sh %{_datadir}/animecix/post-remove-cleanup.sh || :
fi

%files
%{_bindir}/animecix
%{_bindir}/animecix-calendar
%{_bindir}/yggstack
%{_datadir}/applications/tr.com.animecix.desktop
%{_datadir}/icons/hicolor/*/apps/tr.com.animecix.png
%{_datadir}/animecix

%changelog
* Tue Nov 17 2026 veilzon <veilzon@users.noreply.github.com> - 1.4.0-1
- AnimeciX 1.4.0 COPR paketi
- Gomulu libmpv oynatici varsayilan; harici mpv secenegi
- Fansub/kaynak secimi, Mega.nz destegi ve otomatik yedek kaynak
- Intro/outro atlama, otomatik outro atlama ve bolum cekmecesi
- Onbellek gostergesi, yukleme dongenci ve seek geri bildirimi
- Minimal AppImage, cok sezonlu bolum adlari ve tanitim videolari

* Sun Nov 15 2026 veilzon <veilzon@users.noreply.github.com> - 1.3.5-1
- AnimeciX 1.3.5 COPR paketi
- Flatpak ytdl-format oynatma düzeltmesi
