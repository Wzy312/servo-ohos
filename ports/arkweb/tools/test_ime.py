# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""On-device smoke test for the Servo ArkWeb soft-keyboard IME integration.

Run against a device that already has the Servo shim deployed and the Servo engine
active (see ``deploy.py``). It launches the arkweb-test app on a self-describing
``data:`` page, drives the soft keyboard, and asserts:

  * focusing an ``<input>`` raises the keyboard (engine IME attach) and ACE performs
    keyboard-avoidance layout (``NeedSoftKeyboard`` -> web viewport shrinks);
  * typed text reaches the Servo ``<input>`` (the page recolours its background when the
    field equals the expected value, so the assertion is an OCR-free pixel check);
  * backspace edits the field;
  * blurring hides the keyboard AND the page repaints to full height (regression guard
    for the resize stale-frame bug);
  * nothing crashes.

Because Servo renders web content to a GPU surface (invisible to the ArkUI component
tree / UiTest), web-content state is observed two ways: through **hilog markers** for the
IME/ACE plumbing and through **screenshot pixel checks** for anything that requires seeing
what Servo actually painted.

Provisioning (this is a device-lab test, not stock CI):
  * a rooted device with ``deploy.py`` already run (shim at ``/system/lib64``);
  * ``web.engine.enforce=100`` for the boot session (the fixture sets it if unset);
  * the default input method installed and enabled (e.g. ``com.example.kikakeyboard``).

Run:  ``uv run ports/arkweb/tools/test_ime.py``   (or ``uv run pytest .../test_ime.py -v``)

TODO(test-infra): this is a hand-rolled hdc harness chosen for zero external
dependencies and because it works against a plain-OHOS device today. Investigate
migrating to a proper host-side UI framework once the tooling is available/validated
for this device:
  * DevEco Testing **hypium** (Python): nicer UI driving + host-side ``capture_screen`` and
    CV image-diff helpers, but it is a Huawei-distributed package (not in the open OHOS
    tree) and still cannot see the web DOM (component-tree based) -> web assertions would
    stay JS/screenshot based.
  * **xDevice** (open, in ``test/testfwk/xdevice``): structured suites/reports/device mgmt.
  * On-device **@ohos/hypium** (ArkTS, already scaffolded in arkweb-test ``ohosTest/``):
    ``Driver.screenCapture`` + ``@ohos.multimedia.image`` for in-test pixel checks, run via
    ``hvigor test``. Good if the suite should live with the app.
Also TODO: replace the layout-dependent soft-keyboard key taps (see ``KEY``) with a
layout-independent text-injection path, and add an exact-string read-back channel (e.g.
routing web ``console.log`` to hilog behind a debug flag, or a test-only JS-eval hook).
"""

from __future__ import annotations

import base64
import os
import re
import shlex
import time
from pathlib import Path

import pytest
from hdc_py import Hdc, HarmonyDevice
from PIL import Image

BUNDLE = "org.openharmonyrs.arkwebtest"
ABILITY = "EntryAbility"
SHIM_PATH = "/system/lib64/libservo_arkweb.so"
ENGINE_ENFORCE_SERVO = "100"

# --- Device-specific coordinates (rk3568, 720x1280, default kika keyboard) -------------
# The <input>/blur points come from the test page layout and are stable. The soft-keyboard
# KEY / BACKSPACE points are LAYOUT-DEPENDENT (IME app + resolution) and will need updating
# elsewhere -- see the module TODO about layout-independent text injection.
INPUT_XY = (360, 253)  # the <input> element in TEST_PAGE
BLUR_XY = (360, 500)  # empty body area (below the input) to blur the field
KEY = {"h": (431, 920), "i": (537, 812)}  # kika key centres
BACKSPACE_XY = (663, 1035)
KEYCODE_BACK = 2  # OHOS MMI KEYCODE_BACK

# Pixel-probe points (whole-screen coords). The page paints its <body> background a colour
# that encodes the field state, so a pixel sample is an OCR-free value assertion.
PROBE_VISIBLE = (360, 500)  # in the web area whether or not the keyboard is up
PROBE_LOW = (360, 1000)  # only inside the web area when it is full-height (keyboard hidden)

EXPECT = "hi"

# <body> background: light when empty, green when the field == EXPECT, red otherwise.
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


# --- hdc helpers -----------------------------------------------------------------------


def sh(device: HarmonyDevice, command: str, check: bool = True) -> str:
    return device.cmd(command, capture_output=True, text=True, check=check).stdout


def tap(device: HarmonyDevice, xy: tuple[int, int]) -> None:
    sh(device, f"uinput -T -c {xy[0]} {xy[1]}", check=False)


def key(device: HarmonyDevice, code: int) -> None:
    sh(device, f"uinput -K -d {code} -u {code}", check=False)


def clear_log(device: HarmonyDevice) -> None:
    sh(device, "hilog -r", check=False)


def read_log(device: HarmonyDevice) -> str:
    # `hilog -x` prints the current buffer and exits (unlike bare `hilog`, which streams).
    return sh(device, "hilog -x", check=False)


def wait_log(device: HarmonyDevice, needle: str, timeout: float = 12.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if needle in read_log(device):
            return True
        time.sleep(0.5)
    return False


def launch_page(device: HarmonyDevice, html: str) -> None:
    url = "data:text/html;base64," + base64.b64encode(html.encode()).decode()
    sh(device, f"aa start -a {ABILITY} -b {BUNDLE} -U {shlex.quote(url)}", check=False)


def screencap(device: HarmonyDevice, dest_dir: Path) -> Image.Image:
    out = sh(device, "snapshot_display", check=False)
    match = re.search(r"(/data/local/tmp/snapshot_[^\s]+\.jpeg)", out)
    assert match, f"could not find snapshot path in: {out!r}"
    device_path = match.group(1)
    local = dest_dir / os.path.basename(device_path)
    device.recv_file(device_path, str(local))
    return Image.open(local).convert("RGB")


# --- pixel classification (JPEG-tolerant; vote over a small neighbourhood) --------------


def _sample(img: Image.Image, xy: tuple[int, int]) -> tuple[int, int, int]:
    x, y = xy
    xs = [max(0, x - 4), x, min(img.width - 1, x + 4)]
    ys = [max(0, y - 4), y, min(img.height - 1, y + 4)]
    pixels = [img.getpixel((sx, sy)) for sx in xs for sy in ys]
    n = len(pixels)
    return tuple(sum(p[c] for p in pixels) // n for c in range(3))  # type: ignore[return-value]


def is_green(rgb: tuple[int, int, int]) -> bool:
    r, g, b = rgb
    return g > 120 and g - r > 40 and g - b > 40


def is_red(rgb: tuple[int, int, int]) -> bool:
    r, g, b = rgb
    return r > 120 and r - g > 40 and r - b > 40


def is_light(rgb: tuple[int, int, int]) -> bool:
    return min(rgb) > 170  # the empty-field light background


# --- fixtures --------------------------------------------------------------------------


@pytest.fixture(scope="session")
def device() -> HarmonyDevice:
    hdc = Hdc()
    targets = hdc.list_targets()
    if not targets:
        pytest.skip("no hdc device connected")
    dev = hdc.connect(targets[0])

    if "__HDC_MISSING__" in sh(
        dev,
        f"if [ -f {SHIM_PATH} ]; then echo __HDC_OK__; else echo __HDC_MISSING__; fi",
        check=False,
    ):
        pytest.skip(f"Servo shim not deployed at {SHIM_PATH} -- run deploy.py first")

    # The engine-select param is not persistent; it resets to the default on reboot. Set it
    # for this boot session if needed (the shim is only exercised when it is 100).
    if sh(dev, "param get web.engine.enforce", check=False).strip() != ENGINE_ENFORCE_SERVO:
        sh(dev, f"param set web.engine.enforce {ENGINE_ENFORCE_SERVO}", check=False)

    # Keep the screen awake and unlocked so the app stays foreground.
    sh(dev, "power-shell wakeup", check=False)
    sh(dev, "power-shell timeout -o 3600000", check=False)
    sh(dev, "uinput -T -m 360 1100 360 250 200", check=False)  # swipe-up unlock (no PIN)
    return dev


_TMP: Path | None = None


@pytest.fixture(scope="module")
def app(device: HarmonyDevice, tmp_path_factory: pytest.TempPathFactory) -> HarmonyDevice:
    global _TMP
    _TMP = tmp_path_factory.mktemp("ime")
    sh(device, f"aa force-stop {BUNDLE}", check=False)
    clear_log(device)
    launch_page(device, TEST_PAGE)
    if not wait_log(device, "[arkweb] built webview", timeout=20.0):
        pytest.fail("Servo webview never built -- check the engine is active (enforce=100)")
    # Let the initial page flush + paint.
    assert wait_log(device, "flushing pending load", timeout=10.0)
    time.sleep(2.0)
    yield device
    sh(device, f"aa force-stop {BUNDLE}", check=False)


def _tmp(device: HarmonyDevice) -> Path:
    assert _TMP is not None
    return _TMP


# --- tests (ordered: they share the launched app and drive one IME session) ------------


def test_page_rendered_full_height(app: HarmonyDevice) -> None:
    """Baseline: the page painted, and it fills the full web viewport (keyboard down)."""
    img = screencap(app, _tmp(app))
    assert is_light(_sample(img, PROBE_LOW)), (
        "page background not painted at the bottom of the web area -- initial render failed"
    )


def test_focus_shows_keyboard_and_avoids(app: HarmonyDevice) -> None:
    """Tapping the input attaches the IME (keyboard up) and ACE shrinks the web viewport."""
    clear_log(app)
    tap(app, INPUT_XY)
    assert wait_log(app, "get_text_config", timeout=12.0), "engine IME did not attach on focus"
    # NeedSoftKeyboard -> ACE keyboard-avoidance resizes the web smaller than full height.
    assert wait_log(app, "ProcessVirtualKeyBoard", timeout=12.0)
    log = read_log(app)
    assert "keyboard:0.000000" not in log.split("ProcessVirtualKeyBoard")[-1], (
        "keyboard reported height 0 -- soft keyboard did not show"
    )


def test_typing_enters_text(app: HarmonyDevice) -> None:
    """Keys typed on the soft keyboard reach the Servo <input> (page turns green on 'hi')."""
    for ch in EXPECT:
        tap(app, KEY[ch])
        time.sleep(0.4)
    time.sleep(1.2)
    img = screencap(app, _tmp(app))
    rgb = _sample(img, PROBE_VISIBLE)
    assert is_green(rgb), f"field did not equal {EXPECT!r} after typing (bg={rgb}, expected green)"


def test_backspace_edits_field(app: HarmonyDevice) -> None:
    """Backspace deletes a character (green 'hi' -> red 'h')."""
    tap(app, BACKSPACE_XY)
    time.sleep(1.2)
    img = screencap(app, _tmp(app))
    rgb = _sample(img, PROBE_VISIBLE)
    assert is_red(rgb), f"backspace did not edit the field (bg={rgb}, expected red for 'h')"


def test_blur_hides_keyboard_and_repaints_full_height(app: HarmonyDevice) -> None:
    """Blur hides the keyboard and the page repaints to full height.

    Guards the resize stale-frame regression: the area revealed when the keyboard closes
    must be repainted (here: the red 'h' background), not left stale/blank.
    """
    clear_log(app)
    key(app, KEYCODE_BACK)  # back closes the keyboard (see UpdateTextFieldStatus wiring)
    assert wait_log(app, "keyboard:0.000000", timeout=12.0), "keyboard did not hide on back"
    time.sleep(1.5)
    img = screencap(app, _tmp(app))
    rgb = _sample(img, PROBE_LOW)
    assert is_red(rgb), (
        f"page did not repaint to full height after the keyboard closed (low pixel={rgb}); "
        "this is the resize stale-frame regression"
    )


def test_no_crash(app: HarmonyDevice) -> None:
    """The engine did not panic during the session and the app is still running."""
    assert "[arkweb] servo panic" not in read_log(app), "Servo panicked during the IME session"
    alive = sh(app, f"ps -ef | grep {BUNDLE} | grep -v grep", check=False).strip()
    assert alive, "app process is gone -- it likely crashed"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
