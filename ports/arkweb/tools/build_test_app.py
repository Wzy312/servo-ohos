# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Build, sign and install the vendored arkweb-test HAP (the on-device test app).

The app lives at ``ports/arkweb/test-app``; the pytest smoke tests in this directory drive it
on device. This automates the otherwise-manual HAP recipe.

Prerequisites: a *writable* OpenHarmony command-line-tools SDK (hvigor writes into the SDK
dir). In this container:
  * SDK: ``/home/ubuntu/ohos-sdk-w`` (a writable copy) with an ``<apiVersion>`` symlink,
    e.g. ``ln -sfn default/openharmony /home/ubuntu/ohos-sdk-w/21``
  * hvigorw: ``/opt/command-line-tools/bin/hvigorw``; node: ``/opt/command-line-tools/tool/node``

The container SDK is API 21 while the app targets compileSdkVersion 26, and hvigor's signing
rejects the plaintext test password, so this script temporarily lowers the SDK version and
clears ``signingConfigs`` to build an UNSIGNED HAP, then signs it with the vendored public
OpenHarmony debug certs (``test-app/signing``) and installs it. ``build-profile.json5`` is
restored afterwards.

Run:  ``uv run ports/arkweb/tools/build_test_app.py``  (add ``--no-install`` to only build)
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

APP = Path(__file__).resolve().parent.parent / "test-app"
BUNDLE = "org.openharmonyrs.arkwebtest"


def run(cmd: list[str], cwd: Path | None = None, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(str(c) for c in cmd))
    subprocess.run(cmd, cwd=cwd, env=env, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build/sign/install the arkweb-test HAP.")
    parser.add_argument("--sdk-dir", default="/home/ubuntu/ohos-sdk-w")
    parser.add_argument("--hvigorw", default="/opt/command-line-tools/bin/hvigorw")
    parser.add_argument("--node-dir", default="/opt/command-line-tools/tool/node")
    parser.add_argument("--compile-sdk", default="21", help="API version the SDK provides")
    parser.add_argument("--target", default=None, help="hdc device serial (auto if one device)")
    parser.add_argument("--hdc", default="hdc")
    parser.add_argument("--no-install", action="store_true", help="build + sign only")
    args = parser.parse_args()

    sdk = Path(args.sdk_dir)
    sign_tool = sdk / "default/openharmony/toolchains/lib/hap-sign-tool.jar"
    if not sign_tool.exists():
        candidates = list(sdk.rglob("hap-sign-tool.jar"))
        if candidates:
            sign_tool = candidates[0]
    if not sign_tool.exists():
        sys.exit(f"hap-sign-tool.jar not found under {sdk}")

    profile = APP / "build-profile.json5"
    original = profile.read_text()
    (APP / "local.properties").write_text(f"sdk.dir={sdk}\nnodejs.dir={args.node_dir}\n")

    try:
        # Lower the SDK version and clear signingConfigs so hvigor builds an unsigned HAP.
        munged = original
        munged = re.sub(r'"compileSdkVersion":\s*\d+', f'"compileSdkVersion": {args.compile_sdk}', munged)
        munged = re.sub(r'"targetSdkVersion":\s*\d+', f'"targetSdkVersion": {args.compile_sdk}', munged)
        munged = re.sub(r'"signingConfigs":\s*\[.*?\],', '"signingConfigs": [],', munged, count=1, flags=re.S)
        munged = re.sub(r'\s*"signingConfig":\s*"[^"]*",\n', "\n", munged, count=1)
        profile.write_text(munged)

        env = {**os.environ, "DEVECO_SDK_HOME": str(sdk), "OHOS_BASE_SDK_HOME": str(sdk)}
        run(
            [args.hvigorw, "assembleHap", "--mode", "module", "-p", "product=default", "--no-daemon"],
            cwd=APP,
            env=env,
        )
    finally:
        profile.write_text(original)  # restore compileSdkVersion 26 + signingConfigs

    unsigned = APP / "entry/build/default/outputs/default/entry-default-unsigned.hap"
    if not unsigned.exists():
        sys.exit(f"unsigned HAP not produced at {unsigned}")
    signed = APP / "entry/build/default/outputs/default/entry-signed.hap"
    signing = APP / "signing"
    run(
        [
            "java",
            "-jar",
            str(sign_tool),
            "sign-app",
            "-keyAlias",
            "openharmony application release",
            "-signAlg",
            "SHA256withECDSA",
            "-mode",
            "localSign",
            "-appCertFile",
            str(signing / "OpenHarmonyApplication.pem"),
            "-profileFile",
            str(signing / "profile-debug.p7b"),
            "-keystoreFile",
            str(signing / "OpenHarmony.p12"),
            "-inFile",
            str(unsigned),
            "-outFile",
            str(signed),
            "-keyPwd",
            "123456",
            "-keystorePwd",
            "123456",
        ]
    )
    print(f"signed HAP: {signed}")

    if not args.no_install:
        hdc = [args.hdc]
        if args.target:
            hdc += ["-t", args.target]
        run([*hdc, "shell", "aa", "force-stop", BUNDLE])
        run([*hdc, "install", "-r", str(signed)])
        print("installed.")


if __name__ == "__main__":
    main()
