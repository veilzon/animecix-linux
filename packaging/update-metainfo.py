#!/usr/bin/env python3
"""AppStream/Flatpak metainfo sürüm kaydını Cargo.toml ile eşitler.

Kullanım:
  python3 packaging/update-metainfo.py --check   # CI kapısı
  python3 packaging/update-metainfo.py --write   # sürüm bump sırasında

`--write`, CHANGELOG.md'deki ilgili sürümün ilk maddesini açıklama olarak
kullanır ve yeni `<release>` kaydını en üste ekler. Var olan aynı sürüm
kaydını tekrar eklemez.
"""
import argparse
import datetime
import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
META = ROOT / "packaging" / "flatpak" / "io.github.veilzon.AnimeciX.metainfo.xml"
CHANGELOG = ROOT / "CHANGELOG.md"
CARGO = ROOT / "Cargo.toml"


def cargo_version() -> str:
    for line in CARGO.read_text(encoding="utf-8").splitlines():
        m = re.match(r'\s*version\s*=\s*"([^"]+)"', line)
        if m:
            return m.group(1)
    raise SystemExit("Cargo.toml içinde sürüm bulunamadı")


def latest_release(text: str) -> str | None:
    m = re.search(r'<releases>\s*<release version="([^"]+)"', text)
    return m.group(1) if m else None


def changelog_summary(version: str) -> str:
    if not CHANGELOG.exists():
        return f"AnimeciX {version} sürümü."
    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.startswith("# ") and re.search(rf"v{re.escape(version)}\b", line):
            start = i + 1
            break
    if start is None:
        return f"AnimeciX {version} sürümü."
    block: list[str] = []
    for line in lines[start:]:
        if line.startswith("# "):
            break
        block.append(line)
    item: list[str] = []
    started = False
    for line in block:
        if line.startswith("- "):
            if started:
                break
            started = True
            item.append(line[2:].strip())
        elif started and (line.startswith("  ") or not line.strip()):
            if line.strip():
                item.append(line.strip())
        elif started:
            break
    text = " ".join(item).replace("**", "")
    text = re.sub(r"\s+", " ", text).strip()
    return text[:400] if text else f"AnimeciX {version} sürümü."


def remove_release(text: str, version: str) -> str:
    pattern = re.compile(
        r'[ \t]*<release version="' + re.escape(version) + r'"[^>]*>.*?</release>\n',
        re.S,
    )
    return pattern.sub("", text, count=1)


def main() -> int:
    ap = argparse.ArgumentParser()
    group = ap.add_mutually_exclusive_group()
    group.add_argument("--write", action="store_true", help="metainfo'yu güncelle")
    group.add_argument("--check", action="store_true", help="güncel değilse hata ver")
    args = ap.parse_args()

    version = cargo_version()
    text = META.read_text(encoding="utf-8")
    latest = latest_release(text)

    if latest == version:
        print(f"metainfo güncel: {version}")
        return 0

    if args.check or not args.write:
        print(
            f"HATA: metainfo en son sürüm {latest}, Cargo.toml {version}. "
            "`python3 packaging/update-metainfo.py --write` çalıştırın.",
            file=sys.stderr,
        )
        return 1

    text = remove_release(text, version)
    description = html.escape(changelog_summary(version), quote=False)
    date = datetime.date.today().isoformat()
    entry = (
        f'    <release version="{version}" date="{date}">\n'
        "      <description>\n"
        f"        <p>{description}</p>\n"
        "      </description>\n"
        "    </release>\n"
    )
    marker = "  <releases>\n"
    if marker not in text:
        raise SystemExit("metainfo içinde <releases> bulunamadı")
    text = text.replace(marker, marker + entry, 1)
    META.write_text(text, encoding="utf-8")
    print(f"metainfo güncellendi: {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
