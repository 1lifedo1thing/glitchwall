"""The theme names live in three places (Python, the page, Lively's dropdowns); keep them in step."""
import json
import re
import unittest
from pathlib import Path

from glitchwall import themes

WP = Path(__file__).resolve().parents[1] / "src" / "glitchwall" / "wallpaper"
PAGE = (WP / "index.html").read_text(encoding="utf-8")
PROPS = json.loads((WP / "LivelyProperties.json").read_text(encoding="utf-8"))


def block(start: str) -> str:
    i = PAGE.index(start)
    return PAGE[i:PAGE.index("\n};", i)]


class TestNamesMatch(unittest.TestCase):
    def test_page_themes(self):
        names = re.findall(r"^  (\w+):\s*\{", block("const THEMES = {"), re.M)
        self.assertEqual(names, list(themes.THEMES))

    def test_page_rains(self):
        rains = re.search(r"const RAINS = \[([^\]]*)\]", PAGE).group(1)
        self.assertEqual(["theme"] + re.findall(r"'(\w+)'", rains), list(themes.RAINS))
        for r in themes.RAINS:
            if r != "theme":
                self.assertRegex(PAGE, rf"\n  {r}: \{{", f"no STYLES.{r} renderer")

    def test_page_fonts(self):
        names = re.findall(r"^  (\w+):\s*\"", block("const FONTS = {"), re.M)
        self.assertEqual(["theme"] + names, list(themes.FONTS))

    def test_theme_defaults_are_known(self):
        found = 0
        for m in re.finditer(r"font:'(\w+)', rain:'(\w+)'", block("const THEMES = {")):
            self.assertIn(m.group(1), themes.FONTS)
            self.assertIn(m.group(2), themes.RAINS)
            found += 1
        self.assertEqual(found, len(themes.THEMES))

    def test_lively_dropdowns(self):
        for prop, kind in (("themeSel", "theme"), ("rainSel", "rain"), ("fontSel", "font")):
            self.assertEqual(PROPS[prop]["items"][1:], list(themes.KINDS[kind]))
            self.assertEqual(PROPS[prop]["value"], 0)


class TestLook(unittest.TestCase):
    def test_defaults_and_cleanup(self):
        self.assertEqual(themes.look({}), {"theme": "phosphor", "rain": "theme", "font": "theme"})
        self.assertEqual(themes.look({"theme": "AMBER", "rain": "stars", "font": "<b>"}),
                         {"theme": "amber", "rain": "stars", "font": "theme"})


if __name__ == "__main__":
    unittest.main()
