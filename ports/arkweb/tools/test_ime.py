# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""On-device smoke test for the Servo ArkWeb soft-keyboard IME integration.

Launches the arkweb-test app on a self-describing ``data:`` page and drives the soft
keyboard, asserting: focusing an ``<input>`` raises the keyboard (engine IME attach) and
ACE performs keyboard-avoidance layout; typed text reaches the Servo ``<input>``;
backspace edits it; blurring hides the keyboard AND the page repaints to full height
(regression guard for the resize stale-frame fix); nothing crashes.

The page recolours its ``<body>`` to encode the field value (light when empty, green when
it equals the expected string, red otherwise), so the value assertion is an OCR-free pixel
check. Shared helpers/fixtures and the harness/provisioning notes live in conftest.py.

TODO(test-infra): the soft-keyboard key taps below (``KEY``) are layout-dependent (kika @
720x1280); replace with a layout-independent text-injection path, and add an exact-string
read-back channel. See conftest.py for the hypium/xDevice migration note.

Run:  ``uv run ports/arkweb/tools/test_ime.py``
"""

import time

import pytest

from conftest import (
    back,
    clear_log,
    data_url,
    force_stop,
    is_light,
    is_red,
    launch_page,
    read_log,
    sample,
    tap,
    wait_log,
)

# <input>/blur points come from the page layout and are stable; the soft-keyboard KEY /
# BACKSPACE points are LAYOUT-DEPENDENT (IME app + resolution) -- see the module TODO.
INPUT_XY = (360, 253)
BLUR_XY = (360, 500)
KEY = {"h": (431, 920), "i": (537, 812)}
BACKSPACE_XY = (663, 1035)

PROBE_VISIBLE = (360, 500)  # in the web area whether or not the keyboard is up
PROBE_LOW = (360, 1000)  # only inside the web area when it is full-height (keyboard hidden)

EXPECT = "hi"

TEST_PAGE = f"""<html><head><meta name=viewport content="width=device-width,initial-scale=1"></head>
<body id=b style="margin:0;background:#ddddee;font-family:sans-serif">
<h2 style="margin:8px">Servo IME test</h2>
<input id=i style="width:94%;margin:0 3%;height:110px;font-size:48px;box-sizing:border-box" placeholder="tap here">
<div style="height:1600px"></div>
<script>
var i=document.getElementById('i'), b=document.getElementById('b');
i.addEventListener('input', function() {{
  b.style.background = i.value==='' ? '#ddddee' : (i.value==='{EXPECT}' ? '#00cc00' : '#cc0000');
}});
</script>
</body></html>"""


def is_green(rgb):  # local: the test page's exact green, slightly looser than conftest's
    r, g, b = rgb
    return g > 110 and g - r > 35 and g - b > 35


@pytest.fixture(scope="module")
def app(device):
    """Launch the IME page once; the ordered tests below share one keyboard session."""
    force_stop(device)
    clear_log(device)
    launch_page(device, url=data_url(TEST_PAGE))
    assert wait_log(device, "[arkweb] built webview", 20.0), (
        "Servo webview never built -- is the engine active (enforce=100)?"
    )
    assert wait_log(device, "flushing pending load", 10.0)
    time.sleep(2.0)
    yield device
    force_stop(device)


# The tests are ordered: they share the launched app and drive one IME session.


def test_page_rendered_full_height(app, cap):
    assert is_light(sample(cap(), PROBE_LOW)), "initial render did not paint the full web area"


def test_focus_shows_keyboard_and_avoids(app, cap):
    clear_log(app)
    tap(app, INPUT_XY)
    assert wait_log(app, "get_text_config", 12.0), "engine IME did not attach on focus"
    assert wait_log(app, "ProcessVirtualKeyBoard", 12.0)
    assert "keyboard:0.000000" not in read_log(app).split("ProcessVirtualKeyBoard")[-1], (
        "keyboard reported height 0 -- soft keyboard did not show"
    )


def test_typing_enters_text(app, cap):
    for ch in EXPECT:
        tap(app, KEY[ch])
        time.sleep(0.4)
    time.sleep(1.2)
    rgb = sample(cap(), PROBE_VISIBLE)
    assert is_green(rgb), f"field did not equal {EXPECT!r} after typing (bg={rgb}, expected green)"


def test_backspace_edits_field(app, cap):
    tap(app, BACKSPACE_XY)
    time.sleep(1.2)
    rgb = sample(cap(), PROBE_VISIBLE)
    assert is_red(rgb), f"backspace did not edit the field (bg={rgb}, expected red for 'h')"


def test_blur_hides_keyboard_and_repaints_full_height(app, cap):
    clear_log(app)
    back(app)  # back closes the keyboard (UpdateTextFieldStatus / NeedSoftKeyboard wiring)
    assert wait_log(app, "keyboard:0.000000", 12.0), "keyboard did not hide on back"
    time.sleep(1.5)
    rgb = sample(cap(), PROBE_LOW)
    assert is_red(rgb), (
        f"page did not repaint to full height after the keyboard closed (low pixel={rgb}); "
        "this is the resize stale-frame regression"
    )


def test_no_crash(app):
    assert "[arkweb] servo panic" not in read_log(app), "Servo panicked during the IME session"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
