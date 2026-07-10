# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""URL-bar sync for the Servo ArkWeb backend.

The app's URL bar is an ArkTS `TextInput` kept in sync with the engine. Load-driven updates go
through `onPageBegin`/`onPageEnd`, but history back/forward can reactivate a live pipeline without
a fresh load, so no page-load callback fires. The engine still reports the new URL via Servo's
`notify_url_changed` -> `on_url_changed` -> ArkWeb `OnRefreshAccessedHistory` (ArkTS
`onRefreshAccessedHistory`), which the app also listens to. This test guards that path: after
tapping back, the URL bar must return to page A's URL, not stay on B.
"""

import time

import pytest

from conftest import (
    BACK_BUTTON_XY,
    data_url,
    dump_layout,
    is_green,
    is_red,
    sample,
    tap,
    url_bar_text,
    wait_for_pixel,
)

PROBE = (360, 600)
PAGE_B = data_url("<html><body style='margin:0;background:#cc0000'></body></html>")
PAGE_A = (
    "<html><head><meta name=viewport content='width=device-width,initial-scale=1'></head>"
    "<body style='margin:0;background:#00cc00'>"
    f'<a href="{PAGE_B}" style="display:block;height:100vh"></a>'
    "</body></html>"
)


def wait_url_bar(device, dest_dir, expected: str, timeout: float = 6.0) -> str:
    """Poll the URL bar until it equals `expected` (or timeout); return the last value read."""
    deadline = time.time() + timeout
    last = ""
    while time.time() < deadline:
        last = url_bar_text(dump_layout(device, dest_dir)) or ""
        if last == expected:
            return last
        time.sleep(0.3)
    return last


def test_url_bar_follows_history_back(launch, cap, tmp_path):
    url_a = data_url(PAGE_A)
    device = launch(url=url_a)
    assert is_green(sample(wait_for_pixel(cap, PROBE, is_green), PROBE)), "page A did not render"
    assert wait_url_bar(device, tmp_path, url_a) == url_a, "URL bar wrong after loading page A"

    tap(device, (360, 500))  # tap the viewport-filling link -> page B
    assert is_red(sample(wait_for_pixel(cap, PROBE, is_red), PROBE)), "link tap did not navigate to B"
    assert wait_url_bar(device, tmp_path, PAGE_B) == PAGE_B, "URL bar did not follow link nav to B"

    tap(device, BACK_BUTTON_XY)  # app history back button -> page A
    assert is_green(sample(wait_for_pixel(cap, PROBE, is_green), PROBE)), "back did not return to A"
    assert wait_url_bar(device, tmp_path, url_a) == url_a, (
        "URL bar stale after history back: it must return to page A, not stay on B"
    )


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
