"""The theme names live in two places (Python and the page); keep them in step."""
import re
import unittest
from pathlib import Path

from glitchwall import themes

WP = Path(__file__).resolve().parents[1] / "src" / "glitchwall" / "wallpaper"
PAGE = (WP / "index.html").read_text(encoding="utf-8")


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
        names = re.findall(r"^  (\w+):\s*\[", block("const FONTS = {"), re.M)
        self.assertEqual(["theme"] + names, list(themes.FONTS))

    def test_theme_fields(self):
        body = block("const THEMES = {") + "\n"
        for name in themes.THEMES:
            with self.subTest(name):
                m = re.search(rf"^  {name}:\s*\{{(.*?)\}},\n", body, re.M | re.S)
                self.assertTrue(m)
                f = dict(re.findall(r"(\w+):'([^']*)'", m.group(1)))
                self.assertIn(f["display"], themes.FONTS)
                self.assertIn(f["mono"], themes.FONTS)
                self.assertIn(f["rain"], themes.RAINS)
                self.assertIn(f["mode"], ("light", "dark"))
                for k in ("bg", "surface", "ink", "muted", "faint", "acc", "acc2", "err", "dim", "hi"):
                    self.assertRegex(f[k], r"^#[0-9A-Fa-f]{6}$", k)   # the page's rgb() needs #rrggbb

    def test_every_theme_has_a_transition(self):
        m = re.search(r"const THEME_ANIM = \{(.*?)\};", PAGE, re.S)
        pairs = dict(re.findall(r"(\w+): '(\w+)'", m.group(1)))
        anims = re.findall(r"^  (\w+):\s*\['", block("const ANIMS = {") + "\n", re.M)
        self.assertEqual(sorted(pairs), sorted(themes.THEMES))
        for theme, anim in pairs.items():
            self.assertIn(anim, anims, theme)
        for name in re.findall(r"\['(\w+) ", block("const ANIMS = {")) + re.findall(r", '(\w+) ", block("const ANIMS = {")):
            self.assertIn(f"@keyframes {name}", PAGE)

    def test_transition_names_match(self):
        anims = re.findall(r"^  (\w+):\s*\['", block("const ANIMS = {") + "\n", re.M)
        self.assertEqual(["theme"] + anims, list(themes.TRANSITIONS))
        self.assertEqual(list(themes.PALETTES)[1:], list(themes.THEMES))

    def test_bundled_fonts_exist_with_licenses(self):
        for url in re.findall(r"url\('([^']+\.(?:ttf|otf))'\)", PAGE):
            self.assertTrue((WP / url).is_file(), url)
        ttfs = list((WP / "fonts").glob("*.ttf"))
        self.assertEqual(len(ttfs), 14)
        self.assertEqual(len(list((WP / "fonts" / "licenses").glob("OFL-*.txt"))), len(ttfs))

    def test_themes_use_only_bundled_fonts(self):
        system = {"lucida", "segoe", "mono"}          # menu options that say they are system fonts
        body = block("const THEMES = {") + "\n"
        for name in themes.THEMES:
            m = re.search(rf"^  {name}:\s*\{{(.*?)\}},\n", body, re.M | re.S)
            f = dict(re.findall(r"(\w+):'([^']*)'", m.group(1)))
            for k in ("display", "mono"):
                self.assertNotIn(f[k], system, f"{name}.{k}")


class TestLook(unittest.TestCase):
    def test_defaults_and_cleanup(self):
        self.assertEqual(themes.look({}), {"theme": "phosphor", "palette": "theme", "rain": "theme", "font": "theme", "transition": "theme"})
        self.assertEqual(themes.look({"theme": "AMBER", "palette": "neon", "rain": "stars", "font": "<b>", "transition": "stamp"}),
                         {"theme": "amber", "palette": "neon", "rain": "stars", "font": "theme", "transition": "stamp"})


if __name__ == "__main__":
    unittest.main()
