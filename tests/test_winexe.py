"""glitchwall-feed.exe (Python under glitchwall's name) and the covered-screen check."""
import ctypes
import os
import shutil
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import winexe, winhost

ICON = Path(__file__).resolve().parents[1] / "src" / "glitchwall" / "wallpaper" / "glitchwall.ico"


def description(exe: Path) -> str:
    """FileDescription as Windows reads it (what Task Manager shows)."""
    v = ctypes.windll.version
    n = v.GetFileVersionInfoSizeW(str(exe), None)
    buf = ctypes.create_string_buffer(n)
    v.GetFileVersionInfoW(str(exe), 0, n, buf)
    p, ln = ctypes.c_void_p(), ctypes.c_uint()
    v.VerQueryValueW(buf, r"\StringFileInfo\040904B0\FileDescription", ctypes.byref(p), ctypes.byref(ln))
    return ctypes.wstring_at(p, ln.value).rstrip("\0") if p.value else ""


class TestVersionInfo(unittest.TestCase):
    def test_well_formed(self):
        b = winexe.version_info("glitchwall", "1.2.3")
        length, vlen, wtype = struct.unpack_from("<HHH", b)
        self.assertEqual((length, vlen, wtype), (len(b), 52, 0))
        self.assertEqual(b[6:6 + 32].decode("utf-16-le"), "VS_VERSION_INFO\0")
        fixed = struct.unpack_from("<13I", b, 40)
        self.assertEqual(fixed[0], 0xFEEF04BD)
        self.assertEqual((fixed[2], fixed[3]), (1 << 16 | 2, 3 << 16))
        self.assertIn("glitchwall".encode("utf-16-le"), b)

    def test_no_copy_outside_a_venv(self):
        with mock.patch.object(sys, "prefix", sys.base_prefix):
            self.assertIsNone(winexe.target())


@unittest.skipUnless(os.name == "nt", "Windows only")
class TestBrand(unittest.TestCase):
    def test_copy_says_glitchwall(self):
        d = Path(tempfile.mkdtemp(prefix="gw-exe-"))
        try:
            exe = d / "x.exe"
            shutil.copy2(Path(sys.base_prefix) / "pythonw.exe", exe)
            winexe.brand(exe, ICON, "0.5.0")
            self.assertEqual(description(exe), "glitchwall")
            self.assertEqual(winexe._existing(exe, winexe.RT_GROUP_ICON), [(1, 0x0409)])
        finally:
            shutil.rmtree(d, ignore_errors=True)


class TestCovers(unittest.TestCase):
    def test_covers(self):
        work = (0, 48, 1920, 1080)
        self.assertTrue(winhost.covers((0, 48, 1920, 1080), work))       # maximized
        self.assertTrue(winhost.covers((0, 0, 1920, 1080), work))        # fullscreen
        self.assertFalse(winhost.covers((0, 50, 1920, 1080), work))      # a little short
        self.assertFalse(winhost.covers((302, 131, 1582, 964), work))    # an ordinary window


if __name__ == "__main__":
    unittest.main()
