# /// script
# requires-python = ">=3.9"
# dependencies = ["pytest>=7", "hdc-py>=0.2.0", "pillow>=10"]
# ///
"""JS dialogs (`alert`/`confirm`/`prompt`) for the Servo ArkWeb backend.

Servo surfaces these via `show_embedder_control(EmbedderControl::SimpleDialog)`; the shim
bridges them to ACE's `OnAlertDialogByJS`/`OnConfirmDialogByJS`/`OnPromptDialogByJS` and
routes the user's response back through an `NWebJSDialogResult`.

These tests exercise the round-trip with **no app dialog handler installed** (arkweb-test
sets none), which is the case where ACE reports the dialog unhandled and the shim resolves
it so the page's script is never blocked: `alert()` acknowledges and continues, `confirm()`
returns false, `prompt()` returns null. The page runs the dialog at load and encodes the
outcome as its background colour (OCR-free pixel check). This verifies the full
Servo -> shim -> ACE -> shim -> Servo path is wired and non-blocking.

The *handled* tests at the bottom exercise the ACE->result->Servo callback returning a real
user choice. They require the arkweb-test app's `DialogPage` (launched with `--ps page
dialog`), whose Web component's onConfirm/onAlert/onPrompt show a real ArkUI `AlertDialog`
and return the choice. The dialog's OK/Cancel buttons are ArkUI, so their coordinates come
from `uitest dumpLayout` (not hard-coded); the dialog is triggered on page load via `-U` so
no web-button coordinate is needed. These tests skip if the dialog never appears (DialogPage
not installed). See conftest.py for the harness/provisioning notes.
"""

import time

import pytest

from conftest import data_url, find_center, is_green, is_red, sample, tap


def _page(script: str) -> str:
    return (
        '<html><head><meta name=viewport content="width=device-width,initial-scale=1"></head>'
        '<body id=b style="margin:0;background:#bbbbdd">'
        f"<script>{script}</script></body></html>"
    )


# alert() has no return value: if it resolves, the script continues and paints green.
ALERT = _page("alert('a'); document.getElementById('b').style.background='#00cc00';")
# confirm() with no handler resolves to false -> red.
CONFIRM = _page("document.getElementById('b').style.background = confirm('c') ? '#00cc00' : '#cc0000';")
# prompt() with no handler resolves to null -> green (null === null).
PROMPT = _page("document.getElementById('b').style.background = (prompt('p','d') === null) ? '#00cc00' : '#cc0000';")

PROBE = (360, 600)


def test_alert_resolves_and_continues(launch, cap):
    launch(url=data_url(ALERT))
    time.sleep(3.0)
    assert is_green(sample(cap(), PROBE)), "alert() did not resolve -- the page's script stayed blocked"


def test_confirm_returns_result(launch, cap):
    launch(url=data_url(CONFIRM))
    time.sleep(3.0)
    assert is_red(sample(cap(), PROBE)), "confirm() (no handler) did not return false"


def test_prompt_returns_result(launch, cap):
    launch(url=data_url(PROMPT))
    time.sleep(3.0)
    assert is_green(sample(cap(), PROBE)), "prompt() (no handler) did not return null"


# --- handled path: DialogPage shows a real dialog and returns the user's choice -----------

# Triggered on load so no web-button coordinate is needed; the outcome is encoded as the
# body colour. DialogPage's onPrompt returns the default value on OK.
CONFIRM_LOAD = _page("document.getElementById('b').style.background = confirm('C') ? '#00cc00' : '#cc0000';")
PROMPT_LOAD = _page(
    "document.getElementById('b').style.background = (prompt('P','hi') === 'hi') ? '#00cc00' : '#cc0000';"
)
DIALOG_PROBE = (360, 900)


def _wait_for_button(dump, text: str, timeout: float = 12.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        center = find_center(dump(), text)
        if center is not None:
            return center
        time.sleep(0.5)
    return None


def _skip_if_no_dialog(button):
    if button is None:
        pytest.skip(
            "dialog button never appeared -- run the arkweb-test app's DialogPage "
            "(the HAP with onConfirm/onAlert/onPrompt handlers)"
        )


def test_confirm_ok_returns_true(launch, cap, dump):
    device = launch(url=data_url(CONFIRM_LOAD), page="dialog")
    ok = _wait_for_button(dump, "OK")
    _skip_if_no_dialog(ok)
    tap(device, ok)
    time.sleep(2.5)
    assert is_green(sample(cap(), DIALOG_PROBE)), "confirm() did not return true after OK"


def test_confirm_cancel_returns_false(launch, cap, dump):
    device = launch(url=data_url(CONFIRM_LOAD), page="dialog")
    cancel = _wait_for_button(dump, "Cancel")
    _skip_if_no_dialog(cancel)
    tap(device, cancel)
    time.sleep(2.5)
    assert is_red(sample(cap(), DIALOG_PROBE)), "confirm() did not return false after Cancel"


def test_prompt_ok_returns_value(launch, cap, dump):
    device = launch(url=data_url(PROMPT_LOAD), page="dialog")
    ok = _wait_for_button(dump, "OK")
    _skip_if_no_dialog(ok)
    tap(device, ok)
    time.sleep(2.5)
    assert is_green(sample(cap(), DIALOG_PROBE)), "prompt() did not return the default value after OK"


if __name__ == "__main__":
    import sys

    sys.exit(pytest.main([__file__, "-v"]))
