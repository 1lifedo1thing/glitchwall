"""Fixture files hold the fake secrets the tests feed the redactor. Written out whole, they look like real leaked
keys to code hosts' secret scanners (GitHub refuses the push). So every key-shaped string in a fixture file gets an
invisible marker (U+E000, a private-use character) after its first two characters, which no scanner's pattern
matches. Whatever reads a fixture removes the markers first (renderer/src/redact.rs and logs.rs: UNHIDE).
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))
from glitchwall.redact import KNOWN  # noqa: E402

MARK = ""


def hide_fakes(text: str) -> str:
    """Every match of a known secret format, with MARK after its first two characters. Already hidden text stays."""
    for _ in range(20):   # a match can hold another; repeat until nothing matches
        changed = False
        for rx in KNOWN:
            def mark(m: re.Match) -> str:
                s = m.group(0)
                return s[:2] + MARK + s[2:]
            text, n = rx.subn(mark, text)
            changed = changed or n > 0
        if not changed:
            return text
    raise ValueError("fake secrets still match after 20 passes")
