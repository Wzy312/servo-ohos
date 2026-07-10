# ports/arkweb — Servo as an OpenHarmony ArkWeb backend

A self-contained cdylib (`libservo_arkweb.so`) that implements the ArkWeb `NWeb` C++
interfaces on top of libservo, so a stock ArkTS `Web` component renders with Servo instead
of the Chromium-based engine. A small OpenHarmony diff (captured in `ohos-patches/`) makes
`nweb_helper.cpp` dlopen this library when the system parameter `web.engine.enforce` is `100`.

## Layout

- `src/` — cxx bridge (`bridge.rs`) and the servo event-loop thread + per-webview state
  (`runtime.rs`). NWeb entry points arrive on ACE threads and are forwarded as `Action`s;
  sync getters read a `SyncState` cache written by the `WebViewDelegate` on the servo thread.
- `cpp/` — the hand-written NWeb subclasses (`servo_nweb*.{h,cpp}`, manager stubs, handler
  proxy) over generated no-op stub bases in `cpp/generated/`.
- `vendor/ohos_nweb/` — vendored NWeb headers; `OHOS_SYNC_INFO.md` records the source
  commit. Resync with `tools/sync-headers.sh`; regenerate stubs with `tools/gen_stubs.py`.
- `ohos-patches/` — the OpenHarmony webview diff (engine-select + dlopen branch).
- `test-app/` — vendored ArkTS browser-wrapper HAP the on-device tests drive
  (pages: `Index`, `DialogPage`, `SplitPage`, `TabsPage`).
- `tools/` — build/deploy/test tooling; all Python is PEP 723 and run with `uv run`.

## Build & check

```sh
./mach check --ohos -p arkweb          # host type-check + shim C++ compile (OHOS sysroot)
./mach build --ohos --release --no-package -p arkweb
python3 ports/arkweb/tools/gen_stubs.py --check
```

`mach build -p arkweb` exits non-zero at a post-build binary lookup even on success; assert
that the `.so` exists in `<target>/aarch64-unknown-linux-ohos/release/` instead.

## Deploy

`uv run ports/arkweb/tools/deploy.py --servo-lib <path-to-.so>` pushes (sha256-verified) and
sets the engine parameters. Shim-only iteration needs no reboot: force-stop the test app,
re-run deploy with `--no-reboot --no-enforce`, relaunch. The test app is built, signed and
installed by `uv run ports/arkweb/tools/build_test_app.py` (see its docstring for SDK
prerequisites).

Deploying refuses devices running a vendor distribution of OpenHarmony (HarmonyOS and the
like), since the shim is only reachable through the SERVO branch of a patched
`nweb_helper` and those devices ship their own ArkWeb. `--allow-non-openharmony` overrides.

## On-device tests

The suite in `tools/test_*.py` is pytest-based and drives the test app on a connected
device (developed against a rk3568 flashed from the checkout recorded in
`vendor/ohos_nweb/OHOS_SYNC_INFO.md`, with the matching patched OHOS webview libs deployed).

```sh
# all tests
uv run --with pytest --with hdc-py --with pillow python -m pytest ports/arkweb/tools
# one file
uv run ports/arkweb/tools/test_scroll.py
```

The session fixture skips everything when no device is connected or the shim is not
deployed, sets `web.engine.enforce=100` if unset (not persistent across reboots), and wakes
and unlocks the screen. The IME tests additionally need a default input method on the
device.

### Adding a test

Add a `test_<feature>.py` next to the others (copy the PEP 723 header from an existing
file); shared fixtures and helpers live in `conftest.py`. One file per feature area.

Servo's web content is a GPU surface that UiTest/ArkUI inspection cannot see into, so tests
observe state through two oracles:

- **hilog markers** — engine/plumbing logs (`[arkweb] …`) and page-side
  `console.log`/`console.info` (forwarded through the wired console handler). Use
  `wait_log(device, needle)`, never a bare sleep.
- **screenshot pixel checks** — make the test page (a `data_url(...)` fixture) encode the
  JS-observable value under test as its `<body>` background colour, then assert with
  `sample`/`is_green`/`is_red`/`is_blue`. Repaints present a frame or two after an
  interaction: capture with `wait_for_pixel`/`wait_until_stable`, not `sleep(); cap()`.

For ArkUI chrome (app buttons, dialogs, the URL bar), locate components with
`dump_layout` + `find_center` and drive them with `tap`/`swipe`/`key` — no hard-coded
coordinates. The `launch` fixture accepts `url=` (initial page) and `page=` (test-app page,
e.g. `SplitPage`); it waits for the `built webview` marker and force-stops on teardown.
Every device command is bounded by `CMD_TIMEOUT` so a wedged device fails instead of
hanging the run.

APIs only reachable through `WebviewController` (e.g. `getTitle`, `pageDown`) need a
trigger inside the app: add a button (or page) to `test-app/` that invokes the API and
reports the result via `console.info`, then tap it via `dump_layout` and assert on the log.

Keep this file up to date when the tooling, harness idioms, or test-app hooks change.
