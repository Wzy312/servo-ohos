# OHOS NWeb header vendoring

Verbatim copy of the OpenHarmony NWeb C++ interface headers, synced by
`tools/sync-headers.sh`. **Do not edit by hand** — re-run the sync script instead.

| Field | Value |
| ----- | ----- |
| Source repo | https://gitcode.com/openharmony/web_webview |
| Source path | base/web/webview/ohos_interface/include/ohos_nweb |
| Commit | `f0d95626a5fbd99b5d4106cbdd3785c2934a8152` |
| Commit date | 2026-05-29 10:07:40 +0800 |
| Commit subject | !5178 merge cherry-pick-mr-5175-1779955257202-auto into OpenHarmony-7.0-Beta1 |
| Header count | 68 |

Re-sync with:

```sh
OPENHARMONY_SRC=<checkout> ports/arkweb/tools/sync-headers.sh
```

The vendored commit **must** match the OHOS libraries deployed on the device
(vtable layout depends on it). After syncing, regenerate the stub bases and run
the drift check:

```sh
python3 ports/arkweb/tools/gen_stubs.py
python3 ports/arkweb/tools/gen_stubs.py --check
```

## Toolchain workarounds to re-verify on re-sync

`gen_stubs.py --check` only guards the vtable/pure-virtual surface. These headers also
rely on include order and lenient compiler flags that the shim's `build.rs` compensates
for; re-check them whenever the vendored commit moves:

- **Missing standard includes.** e.g. `nweb_drag_data.h` uses `UINT_MAX` without including
  `<climits>` (it works in the OHOS tree only via transitive includes). `build.rs`
  force-includes `cpp/arkweb_prelude.h` to compensate. If a re-synced header uses another
  unincluded standard symbol, add it to the prelude.
- **Ill-formed enum narrowing.** The same file sets `enum class : unsigned char` (and a
  scoped, int-backed enum) members to `UINT_MAX`, which is a narrowing conversion in a
  converted-constant-expression — ill-formed per the standard, rejected by clang's
  `-Wc++11-narrowing` (error by default). `build.rs` passes `-Wno-c++11-narrowing`.
  Harmless for the MVP (those enums are unused; underlying-type *sizes* are unaffected so
  there is no ABI impact), but if drag-and-drop is ever wired up, confirm the truncated
  enumerator values match the device's.
