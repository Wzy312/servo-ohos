# OpenHarmony webview patches

These patches modify the OpenHarmony `web/webview` component so that Servo can be selected at
runtime as an alternative ArkWeb engine, loaded from `libservo_arkweb.so` (built by this crate).
They are the *only* changes required on the OpenHarmony side — the ACE (`web_delegate.cpp`) and
NAPI layers are untouched.

## Source

- Repo: `https://gitcode.com/openharmony/web_webview` (component `base/web/webview`)
- Branch/tag: `OpenHarmony-7.0-Beta1`
- Base commit: `f0d95626a5fbd99b5d4106cbdd3785c2934a8152`

This matches the commit the NWeb headers in `../vendor/ohos_nweb/` were synced from, so the shim's
vtable layout agrees with the patched libraries. Always build and deploy the shim and the patched
`.z.so` libraries from matching checkouts.

## Applying

From the root of an OpenHarmony `web/webview` checkout at the base commit above:

```sh
cd base/web/webview
git apply /path/to/ports/arkweb/ohos-patches/0001-webview-servo-engine-backend.patch
```

Then build the webview component (arm64) — see `../../../Servo-arkweb-plan.md` §7:

```sh
./build.sh --product-name rk3568 --target-cpu arm64 --build-target webview --ccache
# → out/rk3568/web/webview/{libarkweb_utils.z.so, libarkweb_core_loader.z.so}
```

## What the patch does

- `arkweb_utils/arkweb_utils.h` — adds `SERVO = 100` to `ArkWebEngineVersion` / `ArkWebEngineType`
  and declares `IsActiveWebEngineServo()`.
- `arkweb_utils/arkweb_utils.cpp` — accepts `web.engine.enforce == SERVO` in
  `CalculateActiveWebEngineVersion()`, implements `IsActiveWebEngineServo()`, and adds the `SERVO`
  case to `MapToMetricsVersion()` (so the new enumerator does not trip `-Wswitch -Werror`).
- `ohos_nweb/src/nweb_helper.cpp` — a `LoadServoWebEngine()` helper (dlopen
  `web.engine.servo.path`, default `/system/lib64/libservo_arkweb.so`; ABI-version check; factory
  symbol) and a `SERVO` branch at the top of `NWebHelper::GetWebEngine` that uses it instead of the
  Chromium bridge-helper path.

## Toggle at runtime

```sh
hdc shell param set web.engine.enforce 100   # resets on reboot; re-set before launching the app
```
