#!/usr/bin/env bash
# Sync the OpenHarmony NWeb C++ interface headers into ports/arkweb/vendor/ohos_nweb/.
#
# These headers define the pure-virtual NWebEngine / NWeb / NWebHandler interfaces that the
# Servo ArkWeb shim implements. They are vendored verbatim so the shim builds without an
# OpenHarmony source checkout, and so header/device ABI drift is auditable. Provenance
# (source repo, commit, date) is recorded in vendor/ohos_nweb/OHOS_SYNC_INFO.md.
#
# Usage:
#   OPENHARMONY_SRC=/path/to/openharmony ports/arkweb/tools/sync-headers.sh
set -euo pipefail

OPENHARMONY_SRC="${OPENHARMONY_SRC:-$HOME/openharmony}"
WEBVIEW="$OPENHARMONY_SRC/base/web/webview"
SRC_DIR="$WEBVIEW/ohos_interface/include/ohos_nweb"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_DIR="$SCRIPT_DIR/../vendor/ohos_nweb"

if [ ! -d "$SRC_DIR" ]; then
    echo "error: NWeb headers not found at $SRC_DIR" >&2
    echo "       set OPENHARMONY_SRC to your OpenHarmony checkout root." >&2
    exit 1
fi

COMMIT="$(git -C "$WEBVIEW" rev-parse HEAD)"
DATE="$(git -C "$WEBVIEW" log -1 --format=%ci)"
SUBJECT="$(git -C "$WEBVIEW" log -1 --format=%s)"
REMOTE="$(git -C "$WEBVIEW" remote get-url "$(git -C "$WEBVIEW" remote | head -1)" 2>/dev/null || echo unknown)"

mkdir -p "$VENDOR_DIR"
rm -f "$VENDOR_DIR"/*.h
cp "$SRC_DIR"/*.h "$VENDOR_DIR/"
COUNT="$(find "$VENDOR_DIR" -maxdepth 1 -name '*.h' | wc -l | tr -d ' ')"

cat > "$VENDOR_DIR/OHOS_SYNC_INFO.md" <<EOF
# OHOS NWeb header vendoring

Verbatim copy of the OpenHarmony NWeb C++ interface headers, synced by
\`tools/sync-headers.sh\`. **Do not edit by hand** — re-run the sync script instead.

| Field | Value |
| ----- | ----- |
| Source repo | $REMOTE |
| Source path | base/web/webview/ohos_interface/include/ohos_nweb |
| Commit | \`$COMMIT\` |
| Commit date | $DATE |
| Commit subject | $SUBJECT |
| Header count | $COUNT |

Re-sync with:

\`\`\`sh
OPENHARMONY_SRC=<checkout> ports/arkweb/tools/sync-headers.sh
\`\`\`

The vendored commit **must** match the OHOS libraries deployed on the device
(vtable layout depends on it). After syncing, regenerate the stub bases and run
the drift check:

\`\`\`sh
python3 ports/arkweb/tools/gen_stubs.py
python3 ports/arkweb/tools/gen_stubs.py --check
\`\`\`

## Toolchain workarounds to re-verify on re-sync

\`gen_stubs.py --check\` only guards the vtable/pure-virtual surface. These headers also
rely on include order and lenient compiler flags that the shim's \`build.rs\` compensates
for; re-check them whenever the vendored commit moves:

- **Missing standard includes.** e.g. \`nweb_drag_data.h\` uses \`UINT_MAX\` without including
  \`<climits>\` (it works in the OHOS tree only via transitive includes). \`build.rs\`
  force-includes \`cpp/arkweb_prelude.h\` to compensate. If a re-synced header uses another
  unincluded standard symbol, add it to the prelude.
- **Ill-formed enum narrowing.** The same file sets \`enum class : unsigned char\` (and a
  scoped, int-backed enum) members to \`UINT_MAX\`, which is a narrowing conversion in a
  converted-constant-expression — ill-formed per the standard, rejected by clang's
  \`-Wc++11-narrowing\` (error by default). \`build.rs\` passes \`-Wno-c++11-narrowing\`.
  Harmless for the MVP (those enums are unused; underlying-type *sizes* are unaffected so
  there is no ABI impact), but if drag-and-drop is ever wired up, confirm the truncated
  enumerator values match the device's.
EOF

echo "Synced $COUNT headers -> $VENDOR_DIR (commit ${COMMIT:0:12})"
