"""Shared fixtures and helpers for the ArkWeb on-device smoke tests.

See ``test_ime.py`` for the harness rationale: because Servo renders web content to a
GPU surface that the ArkUI component tree / UiTest cannot see, tests observe state two
ways -- **hilog markers** for engine/ACE plumbing, and **screenshot pixel checks** for
paint. The recurring trick is to make a test page encode a JS-observable value as its
``<body>`` background colour, turning an assertion into an OCR-free pixel check.

Provisioning (device-lab, not stock CI): a rooted device with ``deploy.py`` already run,
``web.engine.enforce=100`` for the boot session (this fixture sets it if unset), and a
default input method installed. The ``device`` fixture skips the suite if the shim is
absent.

Run one file:   ``uv run ports/arkweb/tools/test_scroll.py``
Run all:        ``uv run --with pytest --with hdc-py --with pillow \\
                     python -m pytest ports/arkweb/tools``

TODO(test-infra): shared with test_ime.py -- investigate migrating to host-side hypium /
xDevice, or the on-device @ohos/hypium module, once that tooling is validated here.
"""

from __future__ import annotations

import base64
import http.server
import json
import os
import re
import shlex
import subprocess
import threading
import time
from collections.abc import Callable
from pathlib import Path

import pytest
from hdc_py import Hdc, HarmonyDevice
from PIL import Image

BUNDLE = "org.openharmonyrs.arkwebtest"
ABILITY = "EntryAbility"
SHIM_PATH = "/system/lib64/libservo_arkweb.so"
ENGINE_ENFORCE_SERVO = "100"
KEYCODE_BACK = 2

# App chrome (see arkweb-test Index.ets): the history back-button, and where the web area
# starts on screen (below the URL bar). Device: rk3568, 720x1280.
BACK_BUTTON_XY = (40, 113)
WEB_TOP_Y = 160


# --- hdc helpers -----------------------------------------------------------------------

# Bound every device command. `hdc_py.cmd` forwards kwargs to `subprocess.run`, which has no
# timeout by default, so without this a wedged device -- e.g. a servo bug that hangs the
# ArkUI thread, which blocks `uitest dumpLayout` -- would hang a test *inside* a command and
# never reach the wall-clock deadline in the poll helpers above it. On expiry `subprocess`
# raises TimeoutExpired, failing the test instead of hanging the run. Generous vs. real
# command times (snapshot/dumpLayout ~1-2s); no test command legitimately runs this long.
CMD_TIMEOUT = 30.0


def sh(device: HarmonyDevice, command: str, check: bool = True, timeout: float = CMD_TIMEOUT) -> str:
    # errors="replace": the hilog buffer occasionally contains non-UTF-8 bytes (raw binary in a
    # log line), which would otherwise crash the strict decode inside subprocess and flake any
    # test that reads the log.
    return device.cmd(command, capture_output=True, text=True, errors="replace", check=check, timeout=timeout).stdout


def tap(device: HarmonyDevice, xy: tuple[int, int]) -> None:
    sh(device, f"uinput -T -c {xy[0]} {xy[1]}", check=False)


def swipe(device: HarmonyDevice, x1: int, y1: int, x2: int, y2: int, duration_ms: int = 400) -> None:
    sh(device, f"uinput -T -m {x1} {y1} {x2} {y2} {duration_ms}", check=False)


def key(device: HarmonyDevice, code: int) -> None:
    sh(device, f"uinput -K -d {code} -u {code}", check=False)


def back(device: HarmonyDevice) -> None:
    key(device, KEYCODE_BACK)


def clear_log(device: HarmonyDevice) -> None:
    sh(device, "hilog -r", check=False)


def read_log(device: HarmonyDevice) -> str:
    # `hilog -x` prints the current buffer and exits (bare `hilog` streams).
    return sh(device, "hilog -x", check=False)


def wait_log(device: HarmonyDevice, needle: str, timeout: float = 12.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if needle in read_log(device):
            return True
        time.sleep(0.5)
    return False


def force_stop(device: HarmonyDevice) -> None:
    sh(device, f"aa force-stop {BUNDLE}", check=False)


def data_url(html: str) -> str:
    return "data:text/html;base64," + base64.b64encode(html.encode()).decode()


def launch_page(device: HarmonyDevice, url: str | None = None, page: str | None = None) -> None:
    command = f"aa start -a {ABILITY} -b {BUNDLE}"
    if page:
        command += f" --ps page {page}"
    if url:
        command += f" -U {shlex.quote(url)}"
    sh(device, command, check=False)


def dump_layout(device: HarmonyDevice, dest_dir: Path) -> dict:
    """Dump the ArkUI component tree via `uitest dumpLayout` and return it parsed.

    Lets tests find the on-screen bounds of ArkUI components (e.g. an AlertDialog's buttons)
    instead of hard-coding coordinates. Note: Servo's web content is a GPU surface and is NOT
    in this tree -- only the `Web` container node is -- so web elements still need page-layout
    coordinates.
    """
    out = sh(device, "uitest dumpLayout", check=False)
    match = re.search(r"(/data/local/tmp/layout_\d+\.json)", out)
    assert match, f"could not find layout path in: {out!r}"
    local = Path(dest_dir) / os.path.basename(match.group(1))
    device.recv_file(match.group(1), str(local))
    return json.loads(local.read_text())


def find_center(tree: dict, text: str) -> tuple[int, int] | None:
    """Return the screen centre of the first component whose `text` attribute equals `text`."""
    found: list[tuple[int, int]] = []

    def walk(node: dict) -> None:
        attributes = node.get("attributes", node)
        if attributes.get("text") == text:
            bounds = re.match(r"\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]", attributes.get("bounds", ""))
            if bounds:
                left, top, right, bottom = (int(v) for v in bounds.groups())
                found.append(((left + right) // 2, (top + bottom) // 2))
        for child in node.get("children", []):
            walk(child)

    walk(tree)
    return found[0] if found else None


def web_box(tree: dict) -> tuple[int, int, int, int]:
    """Return the screen bounds (left, top, right, bottom) of the first `Web` component.

    Servo's content is a GPU surface, so this is the only way to know where the web area is
    on pages whose chrome (button grids etc.) pushes it down.
    """
    found: list[tuple[int, int, int, int]] = []

    def walk(node: dict) -> None:
        attributes = node.get("attributes", node)
        if attributes.get("type") == "Web":
            bounds = re.match(r"\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]", attributes.get("bounds", ""))
            if bounds:
                found.append(tuple(int(v) for v in bounds.groups()))  # type: ignore[arg-type]
        for child in node.get("children", []):
            walk(child)

    walk(tree)
    assert found, "no Web component in the layout tree"
    return found[0]


def url_bar_text(tree: dict) -> str | None:
    """Return the text of the app's URL bar (the first `TextInput` in the ArkUI tree)."""

    def walk(node: dict) -> str | None:
        attributes = node.get("attributes", node)
        if attributes.get("type") == "TextInput":
            return attributes.get("text", "")
        for child in node.get("children", []):
            if (found := walk(child)) is not None:
                return found
        return None

    return walk(tree)


def screencap(device: HarmonyDevice, dest_dir: Path) -> Image.Image:
    out = sh(device, "snapshot_display", check=False)
    match = re.search(r"(/data/local/tmp/snapshot_[^\s]+\.jpeg)", out)
    assert match, f"could not find snapshot path in: {out!r}"
    local = Path(dest_dir) / os.path.basename(match.group(1))
    device.recv_file(match.group(1), str(local))
    return Image.open(local).convert("RGB")


# --- settle-aware capture --------------------------------------------------------------
#
# A discrete repaint (a tap toggling a colour, an embedder control resolving) presents a
# frame or two *after* the interaction, so a single sleep-then-cap can race the present and
# read the pre-change frame. That premature-capture race -- not any engine latency -- is what
# made the dialog/select frames read blank (see the servo-arkweb memory). Poll instead.


def wait_for_pixel(
    cap: Callable[[], Image.Image],
    xy: tuple[int, int],
    predicate: Callable[[tuple[int, int, int]], bool],
    timeout: float = 8.0,
    interval: float = 0.3,
) -> Image.Image:
    """Capture until ``sample(img, xy)`` satisfies ``predicate``, or ``timeout`` elapses.

    Returns the matching frame; on timeout returns the last frame so the caller's assertion
    fails against a real colour rather than a stale one. Use instead of ``sleep(); cap()``
    around any interaction whose result appears via a repaint.
    """
    deadline = time.time() + timeout
    while True:
        img = cap()
        if predicate(sample(img, xy)) or time.time() >= deadline:
            return img
        time.sleep(interval)


def wait_until_stable(
    cap: Callable[[], Image.Image],
    box: tuple[int, int, int, int],
    interval: float = 0.4,
    timeout: float = 6.0,
) -> Image.Image:
    """Capture until ``box`` stops changing between consecutive frames (the present settled).

    Useful when the expected colour is not known ahead of time (e.g. waiting for an ACE
    overlay to finish animating in) -- returns the first frame that matches its predecessor.
    """
    deadline = time.time() + timeout
    prev = cap()
    while time.time() < deadline:
        time.sleep(interval)
        cur = cap()
        if not region_changed(prev, cur, box):
            return cur
        prev = cur
    return prev


# --- pixel helpers (JPEG-tolerant; vote over a small neighbourhood) ---------------------


def sample(img: Image.Image, xy: tuple[int, int]) -> tuple[int, int, int]:
    x, y = xy
    xs = [max(0, x - 4), x, min(img.width - 1, x + 4)]
    ys = [max(0, y - 4), y, min(img.height - 1, y + 4)]
    pixels = [img.getpixel((sx, sy)) for sx in xs for sy in ys]
    n = len(pixels)
    return tuple(sum(p[c] for p in pixels) // n for c in range(3))  # type: ignore[return-value]


def is_green(rgb: tuple[int, int, int]) -> bool:
    r, g, b = rgb
    return g > 110 and g - r > 35 and g - b > 35


def is_red(rgb: tuple[int, int, int]) -> bool:
    r, g, b = rgb
    return r > 110 and r - g > 35 and r - b > 35


def is_blue(rgb: tuple[int, int, int]) -> bool:
    r, g, b = rgb
    return b > 110 and b - r > 35 and b - g > 35


def is_light(rgb: tuple[int, int, int]) -> bool:
    return min(rgb) > 170


def region_changed(
    before: Image.Image,
    after: Image.Image,
    box: tuple[int, int, int, int],
    delta: int = 40,
    min_fraction: float = 0.02,
) -> bool:
    """True if more than ``min_fraction`` of pixels in ``box`` changed by > ``delta``.

    Tolerant of JPEG noise (a still frame changes a few LSBs everywhere, well under the
    threshold); a moving element flips a meaningful fraction.
    """
    left, top, right, bottom = box
    b = before.crop(box).getdata()
    a = after.crop(box).getdata()
    changed = sum(1 for pb, pa in zip(b, a) if sum(abs(pb[c] - pa[c]) for c in range(3)) > delta)
    total = (right - left) * (bottom - top)
    return total > 0 and changed / total > min_fraction


# --- fixtures --------------------------------------------------------------------------


@pytest.fixture(scope="session")
def device() -> HarmonyDevice:
    hdc = Hdc()
    targets = hdc.list_targets()
    if not targets:
        pytest.skip("no hdc device connected")
    dev = hdc.connect(targets[0])
    probe = sh(dev, f"if [ -f {SHIM_PATH} ]; then echo __OK__; else echo __MISSING__; fi", check=False)
    if "__MISSING__" in probe:
        pytest.skip(f"Servo shim not deployed at {SHIM_PATH} -- run deploy.py first")
    # The engine-select param is not persistent (resets on reboot); the shim is only
    # exercised when it is 100.
    if sh(dev, "param get web.engine.enforce", check=False).strip() != ENGINE_ENFORCE_SERVO:
        sh(dev, f"param set web.engine.enforce {ENGINE_ENFORCE_SERVO}", check=False)
    sh(dev, "power-shell wakeup", check=False)
    sh(dev, "power-shell timeout -o 3600000", check=False)
    sh(dev, "uinput -T -m 360 1100 360 250 200", check=False)  # swipe-up unlock (no PIN)
    return dev


@pytest.fixture
def cap(device: HarmonyDevice, tmp_path: Path):
    """Return a zero-arg function that captures the screen and returns a PIL image."""
    return lambda: screencap(device, tmp_path)


@pytest.fixture
def dump(device: HarmonyDevice, tmp_path: Path):
    """Return a zero-arg function that dumps and returns the parsed ArkUI component tree."""
    return lambda: dump_layout(device, tmp_path)


# --- local HTTP fixture server (device-reachable via hdc reverse-forward) ----------------

GEO_PAGE = """<html><head><title>geo-page</title></head>
<body style="margin:0;background:#dddddd">
<script>
navigator.geolocation.getCurrentPosition(
  function(p){document.body.style.background='#00cc00';},
  function(e){document.body.style.background=(e.code===1)?'#cc0000':'#2244cc';});
</script></body></html>"""

AUTH_OK_PAGE = "<html><body style='margin:0;background:#00cc00'>authed</body></html>"

# user:passwd -- matches the credentials ControllerPage's onHttpAuthRequest confirms with.
AUTH_CREDENTIALS = "Basic " + base64.b64encode(b"user:passwd").decode()


class _FixtureHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 (BaseHTTPRequestHandler API)
        if self.path == "/geo.html":
            self._page(GEO_PAGE)
        elif self.path == "/auth":
            if self.headers.get("Authorization") == AUTH_CREDENTIALS:
                self._page(AUTH_OK_PAGE)
            else:
                body = b"unauthorized"
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="servo-test"')
                self.send_header("Content-Type", "text/html")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
        else:
            self.send_error(404)

    def _page(self, html: str) -> None:
        body = html.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args) -> None:  # quiet
        pass


@pytest.fixture(scope="session")
def http_server(device: HarmonyDevice):
    """Base URL of a host-side fixture server, reachable from the device at 127.0.0.1.

    An `hdc rport` reverse-forward makes the host server reachable on device loopback.
    Loopback is a *potentially trustworthy* origin (a secure context), which data: URLs are
    not -- required for anything gated on secure contexts, e.g. permission prompts
    (non-secure contexts are silently denied without a prompt). Also serves the Basic-auth
    endpoint (`/auth`, credentials user/passwd) for the HTTP-auth tests.
    """
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _FixtureHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    target = Hdc().list_targets()[0]
    forward = f"tcp:{port}"
    result = subprocess.run(
        ["hdc", "-t", target, "rport", forward, forward],
        capture_output=True,
        text=True,
        timeout=CMD_TIMEOUT,
    )
    assert "OK" in result.stdout or result.returncode == 0, f"hdc rport failed: {result.stdout!r}"
    yield f"http://127.0.0.1:{port}"
    subprocess.run(
        ["hdc", "-t", target, "fport", "rm", forward, forward],
        capture_output=True,
        text=True,
        timeout=CMD_TIMEOUT,
        check=False,
    )
    server.shutdown()


@pytest.fixture
def launch(device: HarmonyDevice):
    """Return a function that (re)launches the app on a page and waits for first render.

    Force-stops the app on teardown so each test starts clean.
    """

    def _launch(url: str | None = None, page: str | None = None, render_timeout: float = 20.0) -> HarmonyDevice:
        force_stop(device)
        clear_log(device)
        launch_page(device, url=url, page=page)
        assert wait_log(device, "[arkweb] built webview", render_timeout), (
            "Servo webview never built -- is the engine active (enforce=100)?"
        )
        return device

    yield _launch
    force_stop(device)
