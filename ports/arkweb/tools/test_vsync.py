# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""Vsync-driven animation for the Servo ArkWeb backend.

Two checks: (1) a CSS animation advances (frames are produced/presented); (2) the frames
are driven by the OHOS display vsync. The vsync check needs no engine change: the shim
names its `OH_NativeVSync` connection `ServoArkWeb-<id>`, and the OHOS Vsync framework logs
that name when it services the connection -- so grepping hilog for it proves the animation
is vsync-driven. See conftest.py for the harness/provisioning notes.
"""

import time

import pytest

from conftest import data_url, read_log, region_changed

# Position/size in vh/vw so screen coordinates are density-independent.
ANIM = """<html><head><meta name=viewport content="width=device-width,initial-scale=1"></head>
<body style="margin:0;background:#111">
<div style="width:12vw;height:12vw;background:#00dd00;position:absolute;top:40vh;animation:m 1.2s linear infinite alternate"></div>
<style>@keyframes m{from{left:2vw}to{left:80vw}}</style>
</body></html>"""

# Screen band the box sweeps through: top:40vh ~= 0.4*1052 + 160 (web offset); box ~12vw tall.
ANIM_BAND = (0, 560, 720, 700)


def test_animation_advances_frames(launch, cap):
    launch(url=data_url(ANIM))
    time.sleep(1.5)
    before = cap()
    time.sleep(0.5)
    after = cap()
    assert region_changed(before, after, ANIM_BAND), (
        "animation did not advance between frames -- frames not being produced/presented"
    )


def test_frames_are_vsync_driven(launch):
    device = launch(url=data_url(ANIM))
    # The OHOS Vsync framework (tag C01400/Vsync) logs our named connection (NativeVsync
    # "ServoArkWeb-<id>") when it services vsync -- proof the frames are vsync-driven,
    # observable without any engine-side instrumentation. The "first vsync" line is logged
    # once and the hilog buffer is busy, so poll a single snapshot rather than reading twice.
    deadline = time.time() + 15.0
    vsync_lines: list[str] = []
    while time.time() < deadline and not vsync_lines:
        vsync_lines = [line for line in read_log(device).splitlines() if "Vsync" in line and "ServoArkWeb-" in line]
        if not vsync_lines:
            time.sleep(0.3)
    assert vsync_lines, (
        "OHOS Vsync framework never serviced the ServoArkWeb connection in hilog -- frames may not be vsync-driven"
    )


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
