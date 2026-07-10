# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""HTTP-auth and permission prompts wired from the Servo delegate to ArkWeb.

HTTP auth (`request_authentication` -> `onHttpAuthRequest`) works end-to-end: a 401 endpoint
on the `http_server` fixture triggers the prompt; ControllerPage's handler confirms with
user/passwd; the credentials are retried and the server serves the authed page. Verified from
pixels.

Permission prompts (`request_permission` -> `onGeolocationShow` / `onPermissionRequest`) are
wired on both the shim and app sides, but not reachable from web content on this Servo build:
Servo's `Geolocation::request_position` never sends `PromptPermission` (steps 5/6 are TODO),
getUserMedia is unimplemented, and even `navigator.storage.persist()` does not reach the
delegate here. So the geolocation round-trip is a tracked xfail -- it will XPASS (alerting us)
once Servo routes geolocation permission to the embedder. See conftest `http_server` for why
fixtures are served over loopback (a secure context; permission prompts require one).
"""

from __future__ import annotations

import pytest

from conftest import is_green, sample, wait_for_pixel, wait_log, web_box


def _web_center(tree: dict) -> tuple[int, int]:
    left, top, right, bottom = web_box(tree)
    return ((left + right) // 2, (top + bottom) // 2)


def test_http_auth_credentials_roundtrip(launch, device, dump, cap, http_server):
    launch(page="controller", url=f"{http_server}/auth")

    # Servo's 401 handling asked the embedder; the shim cannot surface the realm (empty).
    assert wait_log(device, "ArkWebTest: httpAuth=127.0.0.1|"), "onHttpAuthRequest never fired"

    # The confirmed credentials must be retried against the server: only then does it serve
    # the green page.
    point = _web_center(dump())
    img = wait_for_pixel(cap, point, is_green, timeout=15.0)
    assert is_green(sample(img, point)), "credentials round-trip did not reach the authed page"


@pytest.mark.xfail(
    reason="Servo Geolocation::request_position steps 5/6 are TODO: it never sends "
    "PromptPermission, so request_permission -> onGeolocationShow is unreachable. "
    "XPASS here means Servo now routes geolocation permission to the embedder.",
    strict=False,
)
def test_geolocation_permission_prompt(launch, device, http_server):
    launch(page="controller", url=f"{http_server}/geo.html")
    assert wait_log(device, "ArkWebTest: geoShow=http://127.0.0.1"), "onGeolocationShow never fired"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
