# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Link navigation + history back for the Servo ArkWeb backend.

Exercises in-engine link navigation and the app's history-back button (Load /
NavigateBack / can_go_back). Page A (green) links to page B (red); tapping the link must
render B, and the back button must return to A. Content is checked by colour; the URL bar's
own history-back sync is covered separately in test_urlbar.py. See conftest.py for the
harness/provisioning notes.
"""

import pytest

from conftest import BACK_BUTTON_XY, data_url, is_green, is_red, sample, tap, wait_for_pixel

PROBE = (360, 600)

PAGE_B = data_url("<html><body style='margin:0;background:#cc0000'></body></html>")
PAGE_A = (
    "<html><head><meta name=viewport content='width=device-width,initial-scale=1'></head>"
    "<body style='margin:0;background:#00cc00'>"
    f'<a href="{PAGE_B}" style="display:block;height:100vh"></a>'
    "</body></html>"
)


def test_link_navigation_and_back(launch, cap):
    device = launch(url=data_url(PAGE_A))
    assert is_green(sample(wait_for_pixel(cap, PROBE, is_green), PROBE)), "page A did not render"

    tap(device, (360, 500))  # tap the viewport-filling link
    assert is_red(sample(wait_for_pixel(cap, PROBE, is_red), PROBE)), "link tap did not navigate to page B"

    tap(device, BACK_BUTTON_XY)  # app history back button
    assert is_green(sample(wait_for_pixel(cap, PROBE, is_green), PROBE)), "back did not return to page A"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
