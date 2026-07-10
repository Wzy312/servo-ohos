# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Background/foreground rendering throttle.

Regression guard for the `paused_ || occluded_` throttle composition in `ServoNWeb`
(`OnPause`/`OnContinue` and `OnOccluded`/`OnUnoccluded` both feed `set_throttled`): sending
the app to the background must throttle Servo, foregrounding must unthrottle. Observed via
the `[arkweb] set_throttled` hilog marker.

True `OnOccluded` coverage would need another window fully covering the web surface (the RS
occlusion callback); on the rk3568 that is not reliably triggerable from the harness, so
only the pause path is asserted.
"""

from __future__ import annotations

import pytest

from conftest import clear_log, data_url, key, launch_page, wait_log

KEYCODE_HOME = 1

PAGE = "<html><body style='background:#00cc00'>throttle</body></html>"


def test_background_throttles(launch, device):
    launch(url=data_url(PAGE))
    clear_log(device)

    key(device, KEYCODE_HOME)
    assert wait_log(device, "set_throttled id=1 true"), "backgrounding did not throttle Servo"

    launch_page(device)  # foreground again (no force-stop: same process, OnContinue path)
    assert wait_log(device, "set_throttled id=1 false"), "foregrounding did not unthrottle Servo"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
