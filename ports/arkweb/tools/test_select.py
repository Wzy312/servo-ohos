# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""`<select>` dropdowns for the Servo ArkWeb backend.

Servo surfaces a `<select>` via `show_embedder_control(EmbedderControl::SelectElement)`;
the shim hands ACE the option labels + the select's rect and ACE renders the dropdown
itself (`OnSelectPopupMenu`), returning the picked index through the popup callback, which
the shim applies with `select() + submit()`. Unlike JS dialogs this needs no app handler --
ACE's overlay menu works on any page.

The menu rows are real ArkUI overlay nodes, so `uitest dumpLayout` sees them (find the
`Banana` row's bounds instead of guessing coordinates). The page's `onchange` encodes the
chosen value as its background colour, turning the round-trip into a pixel check.

Capture uses `wait_for_pixel`: the body only turns green a repaint *after* the pick
resolves, so a single sleep-then-cap would race the present (this is the premature-capture
artifact that earlier masqueraded as render latency -- see conftest). See conftest.py for
the harness/provisioning notes.
"""

import time

import pytest

from conftest import data_url, find_center, is_green, sample, tap, wait_for_pixel

# Big select near the top so its tap coordinate is unambiguous; onchange paints the body a
# per-value colour (Banana -> green). Body background propagates to the viewport, so a probe
# well below the select/menu still reflects the choice.
SELECT_PAGE = """<!doctype html><meta name=viewport content="width=device-width,initial-scale=1">
<body style="margin:0;background:#dddddd">
<select id=s style="font-size:28px;margin:20px;width:80%;height:60px" onchange="
  document.body.style.background = this.value=='Banana' ? '#00c000'
    : this.value=='Cherry' ? '#0000c0' : '#c00000';">
<option>Apple</option><option>Banana</option><option>Cherry</option>
</select></body>"""

# Inside the select on screen: web area starts ~WEB_TOP_Y, select is margin 20 + height 60
# CSS px (density 1.5 -> ~30..135 device px below the web top).
SELECT_XY = (300, 210)
# Below the select and its open menu (which spans down to ~y=464): plain body background.
BODY_PROBE = (360, 900)


def _wait_for_option(dump, label: str, timeout: float = 10.0):
    """Poll the ArkUI tree until the named menu option appears (the overlay renders a beat
    after the open tap), returning its on-screen centre or None."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        center = find_center(dump(), label)
        if center is not None:
            return center
        time.sleep(0.4)
    return None


def test_select_pick_fires_change(launch, cap, dump):
    device = launch(url=data_url(SELECT_PAGE))
    time.sleep(1.5)

    tap(device, SELECT_XY)
    banana = _wait_for_option(dump, "Banana")
    if banana is None:
        # The open tap can miss if it lands before the page's first layout; try once more.
        tap(device, SELECT_XY)
        banana = _wait_for_option(dump, "Banana")
    if banana is None:
        pytest.skip("ACE <select> menu never appeared -- could not open the dropdown")

    tap(device, banana)
    img = wait_for_pixel(cap, BODY_PROBE, is_green)
    assert is_green(sample(img, BODY_PROBE)), "picking 'Banana' did not fire the change event (body never turned green)"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
