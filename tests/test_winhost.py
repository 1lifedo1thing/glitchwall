"""winhost: the window bookkeeping that runs off the UI thread."""
import os
import unittest
from unittest import mock

from glitchwall import winhost


class Native:
    """Stands in for a pywebview WinForms window."""
    def __init__(self, created: bool):
        self.IsHandleCreated, self.InvokeRequired, self.reads = created, False, 0

    @property
    def Handle(self):
        self.reads += 1
        return 1234


class Window:
    def __init__(self, native):
        self.native = native


@unittest.skipUnless(os.name == "nt", "Windows only")
class TickTest(unittest.TestCase):
    def tick(self, native):
        host = winhost.Host(lambda n: "", "all")
        host.windows[1] = Window(native)
        with mock.patch.object(winhost, "monitors", return_value={1: (0, 0, 100, 100)}), \
             mock.patch.object(winhost, "desktop_layer", return_value=(99, 0)), \
             mock.patch.object(winhost, "_rect", return_value=(0, 0, 1, 1)), \
             mock.patch.object(winhost.u32, "GetParent", return_value=0), \
             mock.patch.object(winhost, "attach") as attach:
            host.tick()
        return host, attach

    def test_window_without_a_handle_is_left_alone(self):
        # reading .Handle now would create the window on this thread: WebView2 then crashes on every resize
        native = Native(created=False)
        host, attach = self.tick(native)
        self.assertEqual(native.reads, 0)
        attach.assert_not_called()
        self.assertEqual(host.attached, {})

    def test_window_with_a_handle_is_attached(self):
        host, attach = self.tick(Native(created=True))
        attach.assert_called_once_with(1234, (0, 0, 100, 100))
        self.assertEqual(host.attached, {1: (99, (0, 0, 100, 100))})


if __name__ == "__main__":
    unittest.main()
