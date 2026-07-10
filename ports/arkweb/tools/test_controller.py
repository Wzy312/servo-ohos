# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""WebviewController API coverage for the wired inbound `NWeb` methods.

Drives the test-app's ControllerPage (`--ps page controller`): a button grid where each
button invokes one `WebviewController` API and reports the outcome to hilog as
``ArkWebTest: <name>=<value>``. Tests tap buttons via the layout dump and assert on the
markers; navigation effects are additionally verified with pixel checks (ground truth,
independent of the getters under test). See conftest.py for harness notes.
"""

from __future__ import annotations

import re
import time

import pytest

from conftest import (
    clear_log,
    find_center,
    is_blue,
    is_green,
    is_red,
    read_log,
    tap,
    wait_for_pixel,
    wait_log,
    web_box,
)

MARKER = re.compile(r"ArkWebTest: (\w+)=(.*)")


def press(device, tree: dict, label: str) -> None:
    center = find_center(tree, label)
    assert center, f"button {label!r} not found in the layout tree"
    tap(device, center)


def invoke(device, tree: dict, label: str, timeout: float = 8.0) -> str:
    """Tap `label` and return the value of its ``ArkWebTest: <label>=...`` marker."""
    clear_log(device)
    press(device, tree, label)
    pattern = re.compile(rf"ArkWebTest: {re.escape(label)}=(.*)")
    deadline = time.time() + timeout
    while time.time() < deadline:
        match = pattern.search(read_log(device))
        if match:
            return match.group(1).strip()
        time.sleep(0.4)
    raise AssertionError(f"no ArkWebTest marker for {label!r} within {timeout}s")


def scroll_y(device, tree: dict) -> float:
    return float(invoke(device, tree, "scrollY"))


def wait_scroll(device, tree: dict, predicate, tries: int = 8) -> float:
    value = scroll_y(device, tree)
    for _ in range(tries):
        if predicate(value):
            return value
        time.sleep(0.5)
        value = scroll_y(device, tree)
    return value


@pytest.fixture
def controller_page(launch, device, dump):
    launch(page="controller")
    assert wait_log(device, "ArkWebTest: pageEnd="), "initial page never finished loading"
    return device, dump()


def test_sync_getters(controller_page):
    device, tree = controller_page
    assert invoke(device, tree, "getTitle") == "ctrl-page"
    assert invoke(device, tree, "getProgress") == "100"
    assert invoke(device, tree, "getUrl").startswith("data:")
    # Servo tracks no pre-redirect URL; the wire returns the committed URL.
    assert invoke(device, tree, "getOrigUrl").startswith("data:")


def test_page_up_down(controller_page):
    device, tree = controller_page
    assert scroll_y(device, tree) == 0

    assert invoke(device, tree, "pageDn") == "ok"
    one_page = wait_scroll(device, tree, lambda v: v > 300)
    assert 300 < one_page < 2000, f"pageDown(false) scrolled {one_page}, want ~one viewport"

    assert invoke(device, tree, "pageDnEnd") == "ok"
    bottom = wait_scroll(device, tree, lambda v: v > 4000)
    assert bottom > 4000, f"pageDown(true) reached {bottom}, want near 6000-viewport"

    assert invoke(device, tree, "pageUp") == "ok"
    up_one = wait_scroll(device, tree, lambda v: v < bottom - 300)
    assert up_one < bottom - 300, f"pageUp(false) reached {up_one}, want ~one viewport above {bottom}"

    assert invoke(device, tree, "pageUpTop") == "ok"
    top = wait_scroll(device, tree, lambda v: v < 5)
    assert top < 5, f"pageUp(true) reached {top}, want 0"


def test_back_or_forward(controller_page, cap):
    device, tree = controller_page
    left, top, right, bottom = web_box(tree)
    center = ((left + right) // 2, (top + bottom) // 2)

    assert invoke(device, tree, "loadA") == "ok"
    assert is_red(wait_for_pixel(cap, center, is_red).getpixel(center)[:3]), "page-a (red) never showed"
    assert invoke(device, tree, "loadB") == "ok"
    wait_for_pixel(cap, center, is_green)
    assert invoke(device, tree, "loadC") == "ok"
    img = wait_for_pixel(cap, center, is_blue)
    assert is_blue(img.getpixel(center)[:3]), "page-c (blue) never showed"

    # backOrForward(-2): C -> A, verified by pixels (ground truth).
    assert invoke(device, tree, "back2") == "ok"
    img = wait_for_pixel(cap, center, is_red)
    assert is_red(img.getpixel(center)[:3]), "backOrForward(-2) did not land on page-a"

    # backOrForward(1): A -> B.
    assert invoke(device, tree, "fwd1") == "ok"
    img = wait_for_pixel(cap, center, is_green)
    assert is_green(img.getpixel(center)[:3]), "backOrForward(1) did not land on page-b"

    # Getters must be fresh after history traversal: servo fires notify_url_changed (and title
    # updates) on back/forward without a fresh load — regression for the c3f4cf6 fix.
    assert invoke(device, tree, "getTitle") == "page-b"


def test_precompile_javascript(controller_page):
    # The wire's contract: the ArkTS promise settles (previously it hung forever). The shim
    # reports "unsupported" as a non-zero code, which the NAPI maps to a rejection.
    device, tree = controller_page
    assert invoke(device, tree, "precompile") == "rejected:-1"


def test_media_playback_state(controller_page):
    # No media session is active on a plain page: state must be NONE (0), and the
    # pause/resume controls must be safely callable. A playing-state transition needs real
    # on-device media playback -- deferred until media fixtures exist.
    device, tree = controller_page
    assert invoke(device, tree, "mediaState") == "0"
    assert invoke(device, tree, "pauseMedia") == "ok"
    assert invoke(device, tree, "resumeMedia") == "ok"
    assert invoke(device, tree, "mediaState") == "0"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
