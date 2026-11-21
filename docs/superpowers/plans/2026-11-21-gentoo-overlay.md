# Gentoo Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** AnimeciX'i Gentoo'da kendi overlay'imizden `emerge media-video/animecix` ile kurulabilir hale getirmek.

**Architecture:** `veilzon/animecix-gentoo` altında tek paketli bir Gentoo overlay'i. Ebuild kaynaktan `cargo.eclass` ile derler; crate URI'leri `Cargo.lock`'tan üretilir, `Manifest` Gentoo `ebuild` ile oluşturulur. CI her push'ta `pkgcheck`/Manifest doğrular, tam `emerge` yalnızca manuel/tag çalışır.

**Tech Stack:** Gentoo Portage, EAPI 8, `cargo.eclass`, `pkgcheck`, Podman (`gentoo/stage3`), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-21-gentoo-overlay-design.md`

## Global Constraints

- Paket adı: `media-video/animecix`
- Sürüm: `1.4.5`, `KEYWORDS="~amd64"`
- Lisans: `MIT`
- Runtime: `gui-libs/gtk:4`, `gui-libs/libadwaita`, `media-video/mpv[libmpv]`, `net-misc/aria2`
- Build: `virtual/rust`, `dev-build/cmake`, `llvm-core/clang`, `dev-lang/perl`, `dev-lang/nasm`, `sys-libs/zlib`
- Kaynak: `https://github.com/veilzon/animecix-linux/archive/refs/tags/v${PV}.tar.gz`
- Kurulum yolu: `/usr/bin`, `/usr/share/applications`, `/usr/share/metainfo`, `/usr/share/icons/hicolor`, `/usr/share/animecix/assets`
- Overlay repo: `veilzon/animecix-gentoo`, varsayılan dal `main`

## Review Focus

- `Cargo.lock` ile ebuild `CRATES` listesi uyuşmazsa derleme ağ kapalıyken patlar; her sürümde liste yeniden üretilmeli.
- `btls-sys` BoringSSL'i `cmake` + `bindgen` + Perl ile derler; eksik build bağımlılığı temiz stage3'te ortaya çıkar.
- `media-video/mpv[libmpv]` kapalıysa uygulama açılışta libmpv bulamaz; bağımlılık USE flag'i zorunlu.
- Desktop/ikon kimliği `tr.com.animecix` olmazsa menüde ikon görünmez.
- `animecix-calendar` kurulmazsa yayın takibi arka plan bildirimleri çalışmaz.

---

### Task 1: Overlay iskeleti

**Files:**
- Create: `/tmp/opencode/animecix-gentoo/metadata/layout.conf`
- Create: `/tmp/opencode/animecix-gentoo/profiles/repo_name`
- Create: `/tmp/opencode/animecix-gentoo/profiles/categories`
- Create: `/tmp/opencode/animecix-gentoo/README.md`

**Interfaces:**
- Produces: `masters = gentoo`, repo adı `animecix`, kategori `media-video`

- [ ] **Step 1: Create directories and metadata**

```bash
mkdir -p /tmp/opencode/animecix-gentoo/{metadata,profiles,media-video/animecix,scripts,.github/workflows}
printf 'masters = gentoo\n' > /tmp/opencode/animecix-gentoo/metadata/layout.conf
printf 'animecix\n' > /tmp/opencode/animecix-gentoo/profiles/repo_name
printf 'media-video\n' > /tmp/opencode/animecix-gentoo/profiles/categories
```

- [ ] **Step 2: Write README with install commands**

README must include:

```bash
sudo eselect repository add animecix git https://github.com/veilzon/animecix-gentoo.git
sudo emaint sync -r animecix
sudo emerge -av media-video/animecix
```

- [ ] **Step 3: Verify skeleton**

Run: `test -f metadata/layout.conf && test -f profiles/repo_name && test -f profiles/categories`
Expected: exit 0

- [ ] **Step 4: Commit**

```bash
cd /tmp/opencode/animecix-gentoo
git init -b main
git add metadata profiles README.md
git commit -m "feat: add Gentoo overlay skeleton"
```

---

### Task 2: 1.4.5 ebuild, metadata and Manifest

**Files:**
- Create: `media-video/animecix/animecix-1.4.5.ebuild`
- Create: `media-video/animecix/metadata.xml`
- Create: `media-video/animecix/Manifest`
- Create: `scripts/gen_crates.py`

**Interfaces:**
- Produces: `animecix-1.4.5.ebuild` with `CRATES` generated from Cargo.lock; `Manifest` for all distfiles.

- [ ] **Step 1: Generate `CRATES` from Cargo.lock**

`scripts/gen_crates.py` parses `Cargo.lock` and prints one `name@version` per line, skipping the `animecix` package itself. Run it against the v1.4.5 tarball's `Cargo.lock`:

```bash
python3 scripts/gen_crates.py /path/to/Cargo.lock > /tmp/opencode/crates.txt
```

Expected: 257 lines, no `animecix@1.4.5`.

- [ ] **Step 2: Write the ebuild**

The ebuild must:

```bash
EAPI=8
CRATES="..."          # generated list, before inherit
inherit cargo

DESCRIPTION="Türkçe anime izleme uygulaması (GTK4/libadwaita + mpv)"
HOMEPAGE="https://github.com/veilzon/animecix-linux"
SRC_URI="https://github.com/veilzon/animecix-linux/archive/refs/tags/v${PV}.tar.gz -> ${P}.tar.gz
	${CARGO_CRATE_URIS}"
S="${WORKDIR}/animecix-linux-${PV}"

LICENSE="MIT"
SLOT="0"
KEYWORDS="~amd64"

RDEPEND="gui-libs/gtk:4
	gui-libs/libadwaita
	media-video/mpv[libmpv]
	net-misc/aria2"
DEPEND="${RDEPEND}"
BDEPEND="${RUST_DEPEND}
	dev-build/cmake
	llvm-core/clang
	dev-lang/perl
	dev-lang/nasm
	sys-libs/zlib"
```

`src_compile` calls `cargo_src_compile`.
`src_install`:

```bash
dobin "$(cargo_target_dir)/animecix" "$(cargo_target_dir)/animecix-calendar"
domenu "${FILESDIR}/animecix.desktop"
insinto /usr/share/metainfo
doins packaging/flatpak/io.github.veilzon.AnimeciX.metainfo.xml
for s in 64 128 256 512; do
	doicon -s "${s}" "assets/icon-${s}.png"
done
insinto /usr/share/animecix
doins assets/ATTRIBUTION-Adwaita.md
cp -r assets/upscale assets/hicolor assets/changelog "${ED}/usr/share/animecix/" || die
```

Add `files/animecix.desktop` with `Exec=animecix`, `Icon=tr.com.animecix`, `StartupWMClass=tr.com.animecix`.

- [ ] **Step 3: Write metadata.xml**

Include upstream maintainer `veilzon`, package description and `remote-id` for GitHub.

- [ ] **Step 4: Generate Manifest in Gentoo container**

Run:

```bash
podman run --rm -v /tmp/opencode/animecix-gentoo:/repo:Z -w /repo/media-video/animecix \
  gentoo/stage3:latest bash -lc \
  'emerge --sync >/dev/null && ebuild animecix-1.4.5.ebuild manifest'
```

Expected: `Manifest` created with tarball and 257 crate entries.

- [ ] **Step 5: Verify Manifest**

Run: `grep -c '^DIST ' media-video/animecix/Manifest`
Expected: `258` (1 source tarball + 257 crates)

- [ ] **Step 6: Commit**

```bash
git add media-video/animecix scripts/gen_crates.py
git commit -m "feat: add AnimeciX 1.4.5 ebuild and Manifest"
```

---

### Task 3: Version bump script

**Files:**
- Create: `scripts/bump.sh`
- Create: `scripts/templates/animecix.ebuild.in`

**Interfaces:**
- Produces: `./scripts/bump.sh v1.4.6` creates `animecix-1.4.6.ebuild`, refreshes `CRATES`, and regenerates `Manifest`.

- [ ] **Step 1: Write the template**

Move the ebuild body to `scripts/templates/animecix.ebuild.in` with `@CRATES@` placeholder.

- [ ] **Step 2: Write `bump.sh`**

The script:

1. Validates `vX.Y.Z` argument.
2. Downloads the tag tarball to a temp dir.
3. Extracts `Cargo.lock` and calls `gen_crates.py`.
4. Substitutes `@PV@`/`@CRATES@` into the template and writes `animecix-${PV}.ebuild`.
5. Runs `ebuild ... manifest` inside the Gentoo container if `ebuild` is absent.
6. Removes the previous ebuild only after the new Manifest succeeds.

- [ ] **Step 3: Test script against current 1.4.5**

Run: `./scripts/bump.sh v1.4.5 --check`
Expected: generated ebuild matches the committed one; exit 0.

- [ ] **Step 4: Commit**

```bash
git add scripts/bump.sh scripts/templates/animecix.ebuild.in
git commit -m "feat: add overlay version bump script"
```

---

### Task 4: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: push/PR `pkgcheck` job and manual/tag full-emerge job.

- [ ] **Step 1: Write `ci.yml`**

Jobs:

- `pkgcheck`: `gentoo/stage3` container, `emerge --sync`, `emerge app-portage/pkgcheck`, `pkgcheck scan --net`.
- `emerge`: only `workflow_dispatch` (tag input) and tag pushes; `ACCEPT_KEYWORDS="~amd64" emerge -av media-video/animecix`, then `animecix --check-icons` and `animecix --check-shaders` under `xvfb-run`.

- [ ] **Step 2: Validate YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml_ok')"`
Expected: `yaml_ok`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add pkgcheck and emerge validation"
```

---

### Task 5: Clean stage3 emerge test

**Files:**
- None expected; fix ebuild if test fails.

- [ ] **Step 1: Run full emerge**

```bash
podman run --rm -v /tmp/opencode/animecix-gentoo:/repo:Z -w /repo \
  gentoo/stage3:latest bash -lc '
    emerge --sync &&
    mkdir -p /etc/portage/repos.conf &&
    printf "[animecix]\nlocation = /repo\n" > /etc/portage/repos.conf/animecix.conf &&
    echo "media-video/animecix ~amd64" >> /etc/portage/package.accept_keywords &&
    emerge -av media-video/animecix
  '
```

Expected: package builds and installs; no missing dependency errors.

- [ ] **Step 2: Smoke test**

Run inside the same container:

```bash
xvfb-run -a animecix --check-icons
xvfb-run -a animecix --check-shaders
animecix-calendar --check
```

Expected: all exit 0.

- [ ] **Step 3: Commit fixes**

```bash
git add -A
git commit -m "fix: address stage3 emerge findings"
```

---

### Task 6: Publish overlay repository

**Files:**
- Create: `/tmp/opencode/animecix-gentoo/.gitignore`

- [ ] **Step 1: Add `.gitignore`**

Ignore `*.tar.gz`, `distfiles/`, `*.ebuild.new`.

- [ ] **Step 2: Create GitHub repo and push**

```bash
cd /tmp/opencode/animecix-gentoo
git add .gitignore
git commit -m "chore: ignore local build artifacts"
gh repo create veilzon/animecix-gentoo --public \
  --description "AnimeciX Gentoo overlay" \
  --source=. --remote=origin --push
```

Expected: repo exists, `main` pushed.

- [ ] **Step 3: Verify remote and README**

Run: `gh repo view veilzon/animecix-gentoo --json url,defaultBranchRef`
Expected: URL present, default branch `main`.

---

### Task 7: Main repo install docs and release notes

**Files:**
- Modify: `README.md` (Arch Linux bölümünden sonra Gentoo)
- Modify: `docs/index.html` (kurulum sekmeleri)
- Modify: `docs/en.html`
- Modify: release notes `v1.4.5` on GitHub

- [ ] **Step 1: Add Gentoo section to README**

```markdown
### Gentoo

Kendi overlay'imiz:

```bash
sudo eselect repository add animecix git https://github.com/veilzon/animecix-gentoo.git
sudo emaint sync -r animecix
sudo emerge -av media-video/animecix
```
```

- [ ] **Step 2: Add Gentoo tab to both docs pages**

Use the same command block; keep the existing download-button style.

- [ ] **Step 3: Update release notes**

Append under installation:

`- **Gentoo:** kendi overlay'imizden \`media-video/animecix\` olarak kurulur.`

Then run:

```bash
gh release edit v1.4.5 -R veilzon/animecix-linux --notes-file /tmp/opencode/v1.4.5-notes.md
```

- [ ] **Step 4: Verify docs**

Run: `rg -n 'animecix-gentoo|media-video/animecix' README.md docs/index.html docs/en.html`
Expected: each file has at least one match.

- [ ] **Step 5: Commit and push**

```bash
git add README.md docs/index.html docs/en.html docs/superpowers/specs/2026-11-21-gentoo-overlay-design.md docs/superpowers/plans/2026-11-21-gentoo-overlay.md
git commit -m "docs: add Gentoo overlay install instructions"
git push origin HEAD:main
```
