# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""`<input type=color>` for the Servo ArkWeb backend.

Servo surfaces the picker via `show_embedder_control(EmbedderControl::ColorPicker)`, but
OHOS/ACE exposes **no colour-chooser hook** (Chromium-based ArkWeb shows the picker inside
the engine itself, so there is nothing to delegate to and no system picker like the file
input gets). The shim therefore resolves it immediately with the input's current value --
non-blocking, colour unchanged -- rather than hanging the page. A real picker would need a
bespoke ArkUI overlay, which belongs in the embedder, not the engine shim.

There is no page-observable signal (submitting the unchanged colour fires no `change`), so
this smoke test asserts the handled-and-resolved path via the shim's hilog marker: tapping an
`<input type=color>` reaches the arm and does not hang or crash. See conftest.py for the
harness notes.
"""

import time

import pytest

from conftest import data_url, tap, wait_log

# Viewport-filling colour input so a centre tap is guaranteed to hit it.
PAGE = """<!doctype html><meta name=viewport content="width=device-width,initial-scale=1">
<body style="margin:0"><input type=color value="#3366cc"
 style="width:100vw;height:100vh"></body>"""

TAP_XY = (360, 640)


def test_color_input_resolves_without_hook(launch):
    device = launch(url=data_url(PAGE))
    time.sleep(1.5)  # let the first layout settle before activating the input
    tap(device, TAP_XY)
    assert wait_log(device, "[arkweb] colour picker"), (
        "tapping <input type=color> did not reach the ColorPicker embedder control"
    )


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
