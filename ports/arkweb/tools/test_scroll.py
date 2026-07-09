# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Touch-drag scrolling for the Servo ArkWeb backend.

Regression guard for the batched-`OnTouchMove` scroll fix (M2): ACE forwards touch-move
through the vector overload, which the shim must forward per point or drags are dropped
and the page does not scroll. The page is a tall stack of coloured stripes; a swipe must
visibly move the content (a plain colour would scroll invisibly, so stripes are used).
See conftest.py for the harness/provisioning notes.
"""

import time

import pytest

from conftest import WEB_TOP_Y, data_url, region_changed, swipe

STRIPES = """<html><head><meta name=viewport content="width=device-width,initial-scale=1"></head>
<body id=b style="margin:0"></body>
<script>
var b=document.getElementById('b');
for (var k=0;k<60;k++){var d=document.createElement('div');d.style.height='80px';d.style.background=(k%2)?'#cc0000':'#00cc00';b.appendChild(d);}
</script></html>"""


def test_touch_drag_scrolls(launch, cap):
    device = launch(url=data_url(STRIPES))
    time.sleep(2.0)
    before = cap()
    # Drag up over the web area -> scroll down; the stripe pattern must shift.
    swipe(device, 360, 950, 360, 300, 500)
    time.sleep(1.5)
    after = cap()
    assert region_changed(before, after, (0, WEB_TOP_Y, 720, 1200)), (
        "touch-drag did not scroll the page (batched OnTouchMove regression?)"
    )


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
