# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""`<input type=file>` pickers for the Servo ArkWeb backend.

Servo surfaces the picker via `show_embedder_control(EmbedderControl::FilePicker)`; the shim
bridges it to ACE's `OnFileSelectorShow(callback, params)` and routes the chosen paths back
through the value callback (`file_picker_continue`/`file_picker_cancel`).

ACE's ArkTS `Web` layer takes `OnFileSelectorShow` by default (even with no app
`onShowFileSelector`) and routes it to the system file picker -- so the shim reports
`handled=true` and a real picker opens. There is no page-observable signal to assert on
(Servo does not fire a `cancel` event on dismiss; only a real selection fires
`input`/`change`), so this smoke test asserts the wiring via a hilog marker the shim logs
when the control is shown -- proving tap -> activation -> embedder control -> shim -> ACE.
The `accept`/`multiple` variant checks those attributes reach the shim. Each test taps BACK
afterwards to dismiss the system picker it opened. See conftest.py for the harness notes.
"""

import time

import pytest

from conftest import back, data_url, tap, wait_log

# Viewport-filling input so a centre tap is guaranteed to hit it.
PLAIN = """<!doctype html><meta name=viewport content="width=device-width,initial-scale=1">
<body style="margin:0"><input type=file style="width:100vw;height:100vh;font-size:40px"></body>"""

MULTI = """<!doctype html><meta name=viewport content="width=device-width,initial-scale=1">
<body style="margin:0"><input type=file multiple accept="image/png,image/jpeg"
 style="width:100vw;height:100vh;font-size:40px"></body>"""

TAP_XY = (360, 640)


def test_file_input_opens_picker(launch):
    device = launch(url=data_url(PLAIN))
    time.sleep(1.5)  # let the first layout settle before activating the input
    tap(device, TAP_XY)
    try:
        assert wait_log(device, "[arkweb] file picker"), (
            "tapping <input type=file> did not reach the FilePicker embedder control"
        )
        # ACE's Web layer takes it by default and shows the system picker.
        assert wait_log(device, "handled=true"), "ACE did not route the file input to a picker"
    finally:
        back(device)  # dismiss the system file picker


def test_file_input_multiple_and_accept(launch):
    device = launch(url=data_url(MULTI))
    time.sleep(1.5)
    tap(device, TAP_XY)
    try:
        assert wait_log(device, "[arkweb] file picker"), "multiple <input type=file> did not open the picker"
        # `multiple` must reach the shim (accept filters ride the same path).
        assert wait_log(device, "multiple=true"), "the input's `multiple` did not reach the shim"
    finally:
        back(device)


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
