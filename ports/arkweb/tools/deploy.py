# /// script
# requires-python = ">=3.9"
# dependencies = ["hdc-py>=0.2.0"]
# ///
"""Deploy the Servo ArkWeb backend to an OpenHarmony device.

Pushes the Servo shim (``libservo_arkweb.so``) plus the patched OHOS webview libraries
(``libarkweb_utils.z.so`` and ``libarkweb_core_loader.z.so``, which carry the SERVO branch in
``arkweb_utils`` / ``nweb_helper.cpp``) into the system image, then sets
``web.engine.enforce=100`` so the next boot selects Servo as the active ArkWeb engine.

Every transfer is sha256-verified host-vs-device by hdc-py's ``send_file`` (a mismatch aborts the
deploy loudly), which guards against the silent truncation ``hdc file send`` can produce when the
device disk is full. Free space on the target partition is checked up front.

Run with uv (installs hdc-py into an ephemeral environment)::

    uv run ports/arkweb/tools/deploy.py \\
        --servo-lib target/aarch64-unknown-linux-ohos/release/libservo_arkweb.so \\
        --ohos-out /home/ubuntu/openharmony/out/rk3568

The OHOS libs may be packaged inside a signed HAP rather than loose on the filesystem; in that
case discovery fails with a clear message and those libs must be replaced by other means.
"""

from __future__ import annotations

import argparse
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path

from hdc_py import Hdc, HarmonyDevice, HdcDisconnectedError

# The two OHOS webview libraries built from the ~3-file diff, by output filename.
OHOS_LIB_NAMES = ("libarkweb_utils.z.so", "libarkweb_core_loader.z.so")

# Servo engine type in arkweb_utils (ArkWebEngineType::SERVO); the value the enforce param takes.
SERVO_ENGINE_TYPE = 100

DEFAULT_SERVO_DEST = "/system/lib64/libservo_arkweb.so"

# Space margin required on the target partition beyond the pushed payload (temp/inode slack).
FREE_SPACE_MARGIN_BYTES = 16 * 1024 * 1024


@dataclass
class Transfer:
    """A single host file to push to a resolved device path."""

    label: str
    host_path: Path
    device_path: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Deploy the Servo ArkWeb backend to an OpenHarmony device.",
    )
    parser.add_argument(
        "--servo-lib",
        type=Path,
        required=True,
        help="Host path to libservo_arkweb.so (the cross-built Servo shim).",
    )
    parser.add_argument(
        "--servo-dest",
        default=DEFAULT_SERVO_DEST,
        help=f"Device path for the Servo shim (default: {DEFAULT_SERVO_DEST}). "
        "Must match the web.engine.servo.path param if that param is overridden.",
    )
    parser.add_argument(
        "--ohos-out",
        type=Path,
        help="OHOS build output dir (e.g. out/rk3568) to locate the patched webview .z.so libs. "
        "Omit to deploy only the Servo shim.",
    )
    parser.add_argument(
        "--ohos-lib",
        action="append",
        default=[],
        metavar="HOST_PATH",
        help="Explicit host path to an OHOS webview .z.so to push (repeatable). Overrides the "
        "automatic search under --ohos-out for that filename.",
    )
    parser.add_argument(
        "--search-root",
        default="/system",
        help="Device root under which to discover existing OHOS lib paths (default: /system).",
    )
    parser.add_argument("--target", help="hdc target serial (auto-detected if exactly one).")
    parser.add_argument("--hdc", type=Path, help="Path to the hdc binary (default: from PATH).")
    parser.add_argument(
        "--no-enforce",
        action="store_true",
        help="Do not set web.engine.enforce (only push libraries).",
    )
    parser.add_argument(
        "--setenforce-permissive",
        action="store_true",
        help="Run 'setenforce 0' so SELinux does not block dlopen of the shim (dev devices only).",
    )
    parser.add_argument(
        "--no-reboot",
        action="store_true",
        help="Do not reboot after deploying (the engine switch takes effect on next boot).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the planned actions without touching the device.",
    )
    return parser.parse_args()


def resolve_device(hdc: Hdc, target: str | None) -> HarmonyDevice:
    if target:
        return hdc.connect(target)
    targets = hdc.list_targets()
    if not targets:
        sys.exit("error: no hdc targets connected")
    if len(targets) > 1:
        sys.exit(f"error: multiple targets connected, pass --target: {', '.join(targets)}")
    return hdc.connect(targets[0])


def device_shell(device: HarmonyDevice, command: str, check: bool = True) -> str:
    """Run a shell command on the device and return its stdout as text."""
    result = device.cmd(command, capture_output=True, text=True, check=check)
    return (result.stdout or "").strip()


def discover_device_path(device: HarmonyDevice, lib_name: str, search_root: str) -> str:
    """Locate an existing library on the device by filename, returning its absolute path."""
    quoted_root = shlex.quote(search_root)
    quoted_name = shlex.quote(lib_name)
    output = device_shell(
        device,
        f"find {quoted_root} -name {quoted_name} -type f 2>/dev/null",
        check=False,
    )
    matches = [line.strip() for line in output.splitlines() if line.strip()]
    if not matches:
        sys.exit(
            f"error: could not locate {lib_name} under {search_root} on the device. "
            "It may be packaged inside a HAP rather than a loose file; replace it by other means."
        )
    if len(matches) > 1:
        print(f"warning: multiple matches for {lib_name}, using the first:", file=sys.stderr)
        for match in matches:
            print(f"    {match}", file=sys.stderr)
    return matches[0]


def build_transfers(args: argparse.Namespace, device: HarmonyDevice) -> list[Transfer]:
    transfers: list[Transfer] = []

    if not args.servo_lib.is_file():
        sys.exit(f"error: --servo-lib not found: {args.servo_lib}")
    transfers.append(Transfer("servo shim", args.servo_lib, args.servo_dest))

    # Map each OHOS lib name to a host path: an explicit --ohos-lib override, else search --ohos-out.
    explicit = {Path(p).name: Path(p) for p in args.ohos_lib}
    if args.ohos_out or explicit:
        for lib_name in OHOS_LIB_NAMES:
            host_path = explicit.get(lib_name)
            if host_path is None and args.ohos_out:
                host_path = find_ohos_lib(args.ohos_out, lib_name)
            if host_path is None:
                print(
                    f"warning: {lib_name} not provided or found; skipping (needed for the SERVO branch to take effect)",
                    file=sys.stderr,
                )
                continue
            if not host_path.is_file():
                sys.exit(f"error: OHOS lib not found: {host_path}")
            device_path = discover_device_path(device, lib_name, args.search_root)
            transfers.append(Transfer(lib_name, host_path, device_path))

    return transfers


def find_ohos_lib(ohos_out: Path, lib_name: str) -> Path | None:
    matches = sorted(ohos_out.rglob(lib_name), key=lambda p: p.stat().st_mtime, reverse=True)
    if not matches:
        return None
    if len(matches) > 1:
        print(f"warning: multiple {lib_name} under {ohos_out}, using newest: {matches[0]}", file=sys.stderr)
    return matches[0]


def check_free_space(device: HarmonyDevice, device_dir: str, needed_bytes: int) -> None:
    quoted_dir = shlex.quote(device_dir)
    output = device_shell(device, f"df -k {quoted_dir}", check=False)
    lines = [line for line in output.splitlines() if line.strip()]
    if len(lines) < 2:
        print(f"warning: could not read free space for {device_dir}", file=sys.stderr)
        return
    fields = lines[-1].split()
    # Toybox df: Filesystem 1K-blocks Used Available Use% Mounted-on
    try:
        available_bytes = int(fields[3]) * 1024
    except (IndexError, ValueError):
        print(f"warning: could not parse df output for {device_dir}: {lines[-1]}", file=sys.stderr)
        return
    if available_bytes < needed_bytes:
        sys.exit(
            f"error: insufficient free space on {device_dir}: "
            f"{available_bytes // 1024} KiB available, {needed_bytes // 1024} KiB required"
        )
    print(f"free space on {device_dir}: {available_bytes // (1024 * 1024)} MiB available")


def main() -> None:
    args = parse_args()

    hdc = Hdc(hdc_path=args.hdc)
    device = resolve_device(hdc, args.target)
    print(f"target: {device.target}")

    transfers = build_transfers(args, device)
    print("planned transfers:")
    for transfer in transfers:
        size = transfer.host_path.stat().st_size
        print(f"  {transfer.label}: {transfer.host_path} -> {transfer.device_path} ({size} bytes)")

    if args.dry_run:
        print("dry run: no changes made")
        return

    device.mount_system_as_rw()

    # Group required free space by target partition top-level dir (crude but adequate).
    needed = sum(t.host_path.stat().st_size for t in transfers) + FREE_SPACE_MARGIN_BYTES
    check_free_space(device, "/system", needed)

    for transfer in transfers:
        print(f"pushing {transfer.label} -> {transfer.device_path}")
        # send_file sha256-verifies the transfer and raises on mismatch (skips if already equal).
        device.send_file(str(transfer.host_path), transfer.device_path)
        print(f"  verified {transfer.device_path}")

    if args.setenforce_permissive:
        print("setting SELinux permissive")
        device_shell(device, "setenforce 0", check=False)

    if not args.no_enforce:
        print(f"setting web.engine.enforce={SERVO_ENGINE_TYPE}")
        device_shell(device, f"param set web.engine.enforce {SERVO_ENGINE_TYPE}")
        if args.servo_dest != DEFAULT_SERVO_DEST:
            print(f"setting web.engine.servo.path={args.servo_dest}")
            device_shell(device, f"param set web.engine.servo.path {shlex.quote(args.servo_dest)}")
        current = device_shell(device, "param get web.engine.enforce", check=False)
        print(f"  web.engine.enforce is now: {current}")

    if not args.no_reboot:
        print("rebooting device")
        # The connection drops as the device reboots. hdc_py's cmd() still tries to read the
        # device-side exit code afterwards and raises once the target disconnects, so swallow it.
        try:
            device.cmd("reboot", check=False)
        except HdcDisconnectedError:
            pass

    print("done")


if __name__ == "__main__":
    main()
