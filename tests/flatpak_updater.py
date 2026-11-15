"""Flatpak guncelleme sozlesmesi: gomulu betik + sahte flatpak-spawn.

Kapsar: `check-flatpak` guncelleme listesini dogru yorumluyor mu,
`update-flatpak` host'ta beklenen komutu calistiriyor mu, `flatpak-spawn`
yokken guvenli davraniyor mu. Gercek Flatpak sandbox'i gerektirmez.
Cikis: 0=PASS, 1=FAIL.
"""
import os
import shutil
import stat
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(ROOT, "packaging", "native-package.sh")
APP = "io.github.veilzon.AnimeciX"

FAKE = """#!/bin/sh
printf '%s\\n' "$*" >> "$CALLS"
case "$1 $2 $3" in
  "--host flatpak remote-ls")
    [ -n "${UPDATES:-}" ] && printf '%s\\n' "$UPDATES"
    exit 0
    ;;
esac
exit 0
"""


class Fail(Exception):
    pass


def check(cond, msg):
    print(("  ok: " if cond else "  FAIL: ") + msg)
    if not cond:
        raise Fail(msg)


def run(env, *args):
    return subprocess.run(
        ["/bin/bash", SCRIPT, *args],
        capture_output=True,
        text=True,
        env=env,
        timeout=30,
    )


def base_env(bindir):
    env = dict(os.environ)
    env["PATH"] = bindir + os.pathsep + env.get("PATH", "")
    env.pop("FLATPAK_ID", None)
    return env


def main():
    if not os.path.isfile(SCRIPT):
        print(f"FAIL: betik yok: {SCRIPT}")
        return 1

    tmp = tempfile.mkdtemp(prefix="flatpak-updater-")
    bindir = os.path.join(tmp, "bin")
    os.makedirs(bindir)
    calls = os.path.join(tmp, "calls")
    fake = os.path.join(bindir, "flatpak-spawn")
    with open(fake, "w") as f:
        f.write(FAKE)
    os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)

    try:
        print("1) flatpak-spawn yokken check/update guvenli...")
        env = base_env(os.path.join(tmp, "empty"))
        os.makedirs(os.path.join(tmp, "empty"), exist_ok=True)
        # PATH'te gercek flatpak-spawn varsa testi anlamsiz kilmasin.
        r = run(env, "check-flatpak")
        check(r.returncode == 1, f"check-flatpak yok -> 1 (exit={r.returncode})")
        r = run(env, "update-flatpak")
        check(r.returncode == 1, f"update-flatpak yok -> 1 (exit={r.returncode})")
        check("flatpak-spawn bulunamadı" in r.stderr, "anlamli hata mesaji")

        print("2) guncelleme listesinde uygulama varsa -> 0...")
        env = base_env(bindir)
        env["CALLS"] = calls
        env["UPDATES"] = f"org.gnome.Platform\n{APP}\norg.freedesktop.Platform"
        r = run(env, "check-flatpak")
        check(r.returncode == 0, f"check-flatpak bulundu -> 0 (exit={r.returncode})")

        print("3) yalnizca runtime guncellemesi varsa -> 1...")
        env["UPDATES"] = "org.gnome.Platform\norg.freedesktop.Platform"
        r = run(env, "check-flatpak")
        check(r.returncode == 1, f"check-flatpak yok -> 1 (exit={r.returncode})")

        print("4) update-flatpak host'ta dogru komutu calistiriyor...")
        open(calls, "w").close()
        env["UPDATES"] = ""
        r = run(env, "update-flatpak")
        check(r.returncode == 0, f"update-flatpak -> 0 (exit={r.returncode})")
        with open(calls) as f:
            got = f.read().strip()
        want = f"--host flatpak update -y {APP}"
        check(got == want, f"komut: {got!r}")

        print("FLATPAK-UPDATER: PASS")
        return 0
    except Fail as e:
        print(f"FLATPAK-UPDATER: FAIL ({e})")
        return 1
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
