"""Copy the license texts of the libraries bundled into the Windows exes into a folder (build time only)."""
import importlib.metadata as md
import shutil
import sys
from pathlib import Path

BUNDLED = ["pywebview", "pythonnet", "clr_loader", "cffi", "pycparser", "bottle", "proxy_tools", "typing_extensions"]
MIT_PROXY_TOOLS = "proxy_tools is distributed under the MIT License (no license file ships with its package).\n"

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
for name in BUNDLED:
    dist = md.distribution(name)
    files = [f for f in (dist.files or []) if any(k in f.name.upper() for k in ("LICENSE", "LICENCE", "COPYING", "AUTHORS"))]
    if not files and name == "proxy_tools":
        (out / f"{name}-LICENSE.txt").write_text(MIT_PROXY_TOOLS, encoding="utf-8")
    for f in files:
        shutil.copy(f.locate(), out / f"{name}-{f.name}.txt")
print(f"licenses copied to {out}: {len(list(out.iterdir()))} files")
