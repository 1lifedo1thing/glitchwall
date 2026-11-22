"""looks.js is the one list of themes, backgrounds, fonts and transitions; check it holds together with the page."""
import re
import unittest
from pathlib import Path

from glitchwall import themes

WP = Path(__file__).resolve().parents[1] / "src" / "glitchwall" / "wallpaper"
PAGE = (WP / "index.html").read_text(encoding="utf-8")
L = themes.LOOKS
SYSTEM_FONTS = {"lucida", "segoe", "mono"}          # menu options that say they are system fonts


class TestLooks(unittest.TestCase):
    def test_page_loads_looks_before_its_script(self):
        self.assertLess(PAGE.index('<script src="looks.js"></script>'), PAGE.index("const LOOKS = window.LOOKS;"))

    def test_every_background_has_a_renderer(self):
        for r in L["backgrounds"]:
            self.assertRegex(PAGE, rf"\n  {r}: \{{", f"no STYLES.{r} renderer")

    def test_theme_fields(self):
        for name, t in L["themes"].items():
            with self.subTest(name):
                self.assertIn(t["display"], L["fonts"])
                self.assertIn(t["mono"], L["fonts"])
                self.assertIn(t["rain"], L["backgrounds"])
                self.assertIn(t["transition"], L["transitions"])
                self.assertIn(t["mode"], ("light", "dark"))
                self.assertTrue(t["about"] and t["palette"])
                for k in ("bg", "surface", "ink", "muted", "faint", "acc", "acc2", "err", "dim", "hi"):
                    self.assertRegex(t[k], r"^#[0-9A-Fa-f]{6}$", k)   # the page's rgb() needs #rrggbb
                for k in ("display", "mono"):
                    self.assertNotIn(t[k], SYSTEM_FONTS, f"{name}.{k}")

    def test_transitions_have_keyframes(self):
        for name, a in L["transitions"].items():
            with self.subTest(name):
                self.assertTrue(a["about"])
                for k in ("in", "out"):
                    self.assertIn(f"@keyframes {a[k].split()[0]} ", PAGE)
                self.assertIn(a.get("place", "any"), ("any", "dock", "stack"))

    def test_fonts(self):
        for name, f in L["fonts"].items():
            with self.subTest(name):
                self.assertTrue(f["about"] and f["family"])
                self.assertGreater(f["scale"], 0)
                self.assertIn(f["weight"], range(100, 1000, 100))

    def test_color_keys_are_theme_fields(self):
        fields = {k for t in L["themes"].values() for k in t}
        for k in L["color_keys"]:
            self.assertTrue(k in fields or k in ("off",), k)

    def test_menus_follow_the_file(self):
        self.assertEqual(list(themes.THEMES), list(L["themes"]))
        self.assertEqual(list(themes.RAINS), ["theme", *L["backgrounds"]])
        self.assertEqual(list(themes.FONTS), ["theme", *L["fonts"]])
        self.assertEqual(list(themes.TRANSITIONS), ["theme", *L["transitions"]])
        self.assertEqual(list(themes.PALETTES)[1:], list(themes.THEMES))

    def test_bundled_fonts_exist_with_licenses(self):
        for url in re.findall(r"url\('([^']+\.(?:ttf|otf))'\)", PAGE):
            self.assertTrue((WP / url).is_file(), url)
        ttfs = list((WP / "fonts").glob("*.ttf"))
        self.assertEqual(len(ttfs), 15)
        self.assertEqual(len(list((WP / "fonts" / "licenses").glob("OFL-*.txt"))), len(ttfs))


class TestLook(unittest.TestCase):
    def test_defaults_and_cleanup(self):
        self.assertEqual(themes.look({}), {"theme": "phosphor", "palette": "theme", "rain": "theme", "font": "theme", "transition": "theme"})
        self.assertEqual(themes.look({"theme": "AMBER", "palette": "neon", "rain": "stars", "font": "<b>", "transition": "stamp"}),
                         {"theme": "amber", "palette": "neon", "rain": "stars", "font": "theme", "transition": "stamp"})


if __name__ == "__main__":
    unittest.main()
