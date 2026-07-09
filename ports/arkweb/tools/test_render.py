# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Basic render/paint correctness for the Servo ArkWeb backend.

A cheap, broad safety net over the whole paint pipeline (WebRender + surfman +
SET_BUFFER_GEOMETRY + vsync): render a page of solid colour bands and assert each band
paints its expected colour. See conftest.py for the harness/provisioning notes.
"""

import time

import pytest

from conftest import data_url, is_blue, is_green, is_red, sample

# Bands sized in vh so their screen positions are density-independent (vh is relative to
# the web viewport, which is the full-height web component when no keyboard is up).
PAGE = """<html><head><meta name=viewport content="width=device-width,initial-scale=1"></head>
<body style="margin:0">
<div style="height:34vh;background:#00cc00"></div>
<div style="height:33vh;background:#cc0000"></div>
<div style="height:33vh;background:#0000cc"></div>
</body></html>"""


def test_renders_solid_colour_bands(launch, cap):
    launch(url=data_url(PAGE))
    time.sleep(2.0)
    img = cap()
    assert is_green(sample(img, (360, 300))), "top band not green -- render pipeline broken"
    assert is_red(sample(img, (360, 650))), "middle band not red"
    assert is_blue(sample(img, (360, 1050))), "bottom band not blue"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
