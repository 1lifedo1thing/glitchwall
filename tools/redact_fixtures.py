"""Record what the Python redactor does, as fixtures the Rust port must match exactly.

Runs the existing test suite with the Redactor wrapped, so every input the tests exercise is recorded with the
settings it ran under and the output it gave. Writes tests/fixtures/redact.json. Run: uv run python tools/redact_fixtures.py
"""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tests"))

from glitchwall import redact  # noqa: E402

seen, cases = set(), []
init, call = redact.Redactor.__init__, redact.Redactor.__call__


def rec_init(self, home=None, user=None, mask_user=True, mask_email=True, entropy_check=True):
    init(self, home, user, mask_user, mask_email, entropy_check)
    # the 8.3 short home it saw (a test fakes one), so the port can be handed the same
    short = redact._short_home(home) if home else ""
    self._cfg = {"home": home, "short": short, "user": user, "mask_user": mask_user, "mask_email": mask_email, "entropy_check": entropy_check}


def rec_call(self, text):
    out = call(self, text)
    cfg = getattr(self, "_cfg", None)
    if cfg and cfg["home"] is not None and cfg["user"] is not None and isinstance(text, str):
        key = (json.dumps(cfg, sort_keys=True), text)
        if key not in seen:
            seen.add(key)
            cases.append({**cfg, "input": text, "output": out})
    return out


redact.Redactor.__init__, redact.Redactor.__call__ = rec_init, rec_call
suite = unittest.defaultTestLoader.discover(str(ROOT / "tests"))
result = unittest.TextTestRunner(verbosity=0, stream=open("nul" if sys.platform == "win32" else "/dev/null", "w")).run(suite)
out = ROOT / "tests" / "fixtures" / "redact.json"
out.write_text(json.dumps(cases, indent=1, ensure_ascii=False), encoding="utf-8")
print(f"{len(cases)} cases from {result.testsRun} tests ({len(result.failures) + len(result.errors)} failed) -> {out.relative_to(ROOT)}")
