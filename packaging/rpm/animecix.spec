Name:           animecix
Version:        1.3.5
Release:        1%{?dist}
Summary:        Türkçe anime, dizi ve film izleme istemcisi
License:        MIT
URL:            https://github.com/veilzon/animecix-linux
Source0:        https://github.com/veilzon/animecix-linux/archive/refs/tags/v%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
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
Requires:       gtk4
Requires:       libadwaita
Requires:       aria2

%description
AnimeciX; GTK4/libadwaita ile yazılmış, mpv tabanlı Türkçe anime,
dizi ve film izleme istemcisidir. İndirme yöneticisi, Discord Rich
Presence, intro/outro atlama ve çevrimdışı oynatma içerir.

%prep
%autosetup -n animecix-linux-%{version}

%build
cargo build --release --locked

%install
install -Dm755 target/release/animecix %{buildroot}%{_bindir}/animecix

mkdir -p %{buildroot}%{_datadir}/animecix/assets
cp -r assets/* %{buildroot}%{_datadir}/animecix/assets/
install -Dm755 packaging/post-install-cleanup.sh \
    %{buildroot}%{_datadir}/animecix/post-install-cleanup.sh

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

%files
%{_bindir}/animecix
%{_datadir}/applications/tr.com.animecix.desktop
%{_datadir}/icons/hicolor/*/apps/tr.com.animecix.png
%{_datadir}/animecix

%changelog
* Sun Nov 15 2026 Yasar Efe Celik <yasar.123.sevda@gmail.com> - 1.3.5-1
- AnimeciX 1.3.5 COPR paketi
- Flatpak ytdl-format oynatma düzeltmesi
