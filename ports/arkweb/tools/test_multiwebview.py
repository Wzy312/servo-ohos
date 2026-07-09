# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Multiple concurrent Servo surfaces (arkweb-test SplitPage).

SplitPage stacks two live `Web` components in one window, exercising concurrent NWeb
creation, two EGL surfaces and two vsync drivers. Its built-in content is a red top
surface (#c0392b "Surface A") and a blue bottom surface (#2471a3 "Surface B"); both must
build and render. See conftest.py for the harness/provisioning notes.
"""

import time

import pytest

from conftest import is_blue, is_red, sample, wait_log


def test_split_page_two_surfaces_render(launch, cap):
    device = launch(page="split")
    assert wait_log(device, "built webview id=2", 15.0), "second Servo surface never built"
    time.sleep(2.5)
    img = cap()

    top = sample(img, (360, 380))
    bottom = sample(img, (360, 980))
    assert is_red(top), f"top surface (A) not red: {top}"
    assert is_blue(bottom), f"bottom surface (B) not blue: {bottom}"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
