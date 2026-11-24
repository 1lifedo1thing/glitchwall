"""Build the release wheel: the Rust renderer inside, labelled Windows-only.

    uv run python tools/build_release.py

Builds the renderer (cargo, release), copies glitchwall-render.exe into src/glitchwall/bin for the length of
the build, builds the wheel with uv, then relabels it from py3-none-any to py3-none-win_amd64: the build
backend only makes pure-Python wheels, and a wheel that carries a Windows exe must not install on macOS or Linux.
Prints the wheel's path and SHA-256. Standard library only. Builds no sdist: one built on another OS would
come out as an any-platform wheel without the renderer.
"""
from __future__ import annotations

import base64
import hashlib
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "renderer" / "target" / "release" / "glitchwall-render.exe"
BIN = ROOT / "src" / "glitchwall" / "bin"
OUT = ROOT / "dist"
TAG = "py3-none-win_amd64"


def run(*cmd: str, cwd: Path = ROOT) -> None:
    print("$", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd, check=True)


def record_hash(data: bytes) -> str:
    return "sha256=" + base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()


def retag(whl: Path) -> Path:
    """Rewrite the wheel with Tag: py3-none-win_amd64 in its WHEEL file (and that file's RECORD line), under the new name."""
    name, version = whl.name.split("-")[:2]
    new = whl.with_name(f"{name}-{version}-{TAG}.whl")
    with zipfile.ZipFile(whl) as zin:
        files = {i.filename: zin.read(i) for i in zin.infolist()}
    info = f"{name}-{version}.dist-info"
    wheel = files[f"{info}/WHEEL"].decode()
    lines = [ln for ln in wheel.splitlines() if not ln.startswith("Tag:")] + [f"Tag: {TAG}"]
    lines = [("Root-Is-Purelib: false" if ln.startswith("Root-Is-Purelib:") else ln) for ln in lines]
    files[f"{info}/WHEEL"] = ("\n".join(lines) + "\n").encode()
    rec = []
    for ln in files[f"{info}/RECORD"].decode().splitlines():
        path = ln.split(",")[0]
        if path == f"{info}/WHEEL":
            data = files[path]
            ln = f"{path},{record_hash(data)},{len(data)}"
        rec.append(ln)
    files[f"{info}/RECORD"] = ("\n".join(rec) + "\n").encode()
    with zipfile.ZipFile(new, "w", zipfile.ZIP_DEFLATED) as zout:
        for path, data in files.items():
            zout.writestr(path, data)
    whl.unlink()
    return new


def main() -> int:
    if os.name != "nt":
        print("build the release on Windows: the renderer is a Windows exe", file=sys.stderr)
        return 1
    run("cargo", "build", "--release", cwd=ROOT / "renderer")
    if BIN.exists():
        shutil.rmtree(BIN)
    BIN.mkdir()
    try:
        shutil.copy2(EXE, BIN / EXE.name)
        OUT.mkdir(exist_ok=True)
        for old in OUT.glob("glitchwall-*.whl"):
            old.unlink()
        run("uv", "build", "--wheel", "-o", str(OUT))
    finally:
        shutil.rmtree(BIN)
    (whl,) = OUT.glob("glitchwall-*-py3-none-any.whl")
    whl = retag(whl)
    with zipfile.ZipFile(whl) as z:
        assert any(n.endswith("bin/glitchwall-render.exe") for n in z.namelist()), "renderer missing from the wheel"
    digest = hashlib.sha256(whl.read_bytes()).hexdigest()
    print(f"\n{whl}\n{whl.stat().st_size / 1e6:.1f} MB  sha256 {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
