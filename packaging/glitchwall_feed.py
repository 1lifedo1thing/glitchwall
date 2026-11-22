"""Entry point for glitchwall-feed.exe (windowed, no console): runs the feed (autostart launches this with no
arguments), or the settings window (`glitchwall-feed.exe settings`, what the Start menu shortcut runs)."""
import sys

from glitchwall.cli import main

sys.exit(main(sys.argv[1:] or ["run"]))
