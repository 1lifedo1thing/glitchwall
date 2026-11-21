"""Entry point for glitchwall-feed.exe (windowed, no console): runs the feed only. Autostart launches this."""
import sys

from glitchwall.cli import main

sys.exit(main(["run"]))
