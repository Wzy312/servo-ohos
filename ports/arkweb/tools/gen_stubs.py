#!/usr/bin/env python3
# Generate concrete-izing stub base classes for the OHOS NWeb C++ interfaces.
#
# The NWeb engine API is a set of pure-virtual C++ classes (NWeb has hundreds of pure
# virtuals). The Servo backend only meaningfully implements a small MVP subset; the rest
# must still be overridden so the classes are instantiable. Rather than hand-write hundreds
# of trivial overrides, this script parses the vendored headers and emits, for each engine
# interface, a `Servo<X>StubBase : public OHOS::NWeb::<X>` that overrides *every* pure
# virtual with a trivial body (`{}` for void, `{ return {}; }` otherwise). The real shim
# classes subclass these bases and override only what they implement.
#
# The generated headers are checked in (under cpp/generated/). `--check` regenerates them
# in memory and fails if they differ from what is on disk -- this is the CI drift tripwire:
# after `sync-headers.sh` pulls a newer OHOS header with a new pure virtual, the checked-in
# stubs go stale (and the shim stops compiling) until this script is re-run.
#
# The parser is deliberately a pragmatic regex/scanner (in the spirit of OHOS's own
# ohos_glue/scripts/file_parser.py), not a full C++ parser. It relies on the interface
# headers being simple: no operator overloads, no reference/function-pointer returns among
# the pure virtuals, signatures are `virtual RET NAME(params) [const] = 0;`.
#
# Usage:
#   python3 ports/arkweb/tools/gen_stubs.py           # (re)generate the stub headers
#   python3 ports/arkweb/tools/gen_stubs.py --check   # verify checked-in output is current

import argparse
import re
import sys
from collections.abc import Iterator
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
ARKWEB_DIR = TOOLS_DIR.parent
VENDOR_DIR = ARKWEB_DIR / "vendor" / "ohos_nweb"
OUT_DIR = ARKWEB_DIR / "cpp" / "generated"

# (vendored header, class to stub, generated base class name, output file).
# We only stub the interfaces the *engine provides*; the input structs ACE hands us
# (NWebCreateInfo, NWebEngineInitArgs, NWebHandler, ...) are consumed, not implemented.
TARGETS = [
    ("nweb.h", "NWeb", "ServoNWebStubBase", "servo_nweb_stub_base.h"),
    ("nweb_engine.h", "NWebEngine", "ServoNWebEngineStubBase", "servo_nweb_engine_stub_base.h"),
    ("nweb_cookie_manager.h", "NWebCookieManager", "ServoCookieManagerStubBase", "servo_cookie_manager_stub_base.h"),
    ("nweb_preference.h", "NWebPreference", "ServoPreferenceStubBase", "servo_preference_stub_base.h"),
    ("nweb_web_storage.h", "NWebWebStorage", "ServoWebStorageStubBase", "servo_web_storage_stub_base.h"),
    ("nweb_data_base.h", "NWebDataBase", "ServoDataBaseStubBase", "servo_data_base_stub_base.h"),
    (
        "nweb_download_manager.h",
        "NWebDownloadManager",
        "ServoDownloadManagerStubBase",
        "servo_download_manager_stub_base.h",
    ),
]

OPEN_BRACKETS = "<([{"
CLOSE_BRACKETS = ">)]}"


def strip_comments(text: str) -> str:
    """Remove C /* */ and // comments. Adequate for these headers (no comment markers
    appear inside string/char literals)."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n]*", "", text)
    return text


def extract_class_body(text: str, class_name: str) -> str:
    """Return the body (between the outermost braces) of `class ... class_name ... { ... }`,
    ignoring forward declarations and other classes that share a name prefix."""
    # class [MACROS...] class_name [: base-clause] {
    pattern = re.compile(r"\bclass\s+(?:[A-Za-z_]\w*\s+)*" + re.escape(class_name) + r"\s*(?::[^{;]*)?\{")
    m = pattern.search(text)
    if not m:
        raise ValueError(f"class {class_name} not found")
    depth = 1
    i = m.end()
    while i < len(text) and depth > 0:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        i += 1
    if depth != 0:
        raise ValueError(f"unbalanced braces in class {class_name}")
    return text[m.end() : i - 1]


def iter_member_decls(body: str) -> Iterator[tuple[str, bool]]:
    """Yield (declaration_text, had_inline_body) for each member declaration in a class body.
    A top-level `{...}` (inline method body or nested type) is consumed and flagged; a
    top-level `;` ends a declaration."""
    body = re.sub(r"\b(public|private|protected)\s*:", " ", body)
    buf = []
    i, n = 0, len(body)
    while i < n:
        c = body[i]
        if c == "{":
            depth, i = 1, i + 1
            while i < n and depth > 0:
                if body[i] == "{":
                    depth += 1
                elif body[i] == "}":
                    depth -= 1
                i += 1
            yield "".join(buf).strip(), True
            buf = []
        elif c == ";":
            yield "".join(buf).strip(), False
            buf = []
            i += 1
        else:
            buf.append(c)
            i += 1
    tail = "".join(buf).strip()
    if tail:
        yield tail, False


def strip_default_args(params: str) -> str:
    """Remove ` = default_value` from each parameter, tracking bracket depth so that
    template/parenthesized commas and defaults are handled."""
    out = []
    depth = 0
    skipping = False
    for c in params:
        if c in OPEN_BRACKETS:
            depth += 1
            if not skipping:
                out.append(c)
        elif c in CLOSE_BRACKETS:
            depth -= 1
            if not skipping:
                out.append(c)
        elif c == "," and depth == 0:
            skipping = False
            out.append(c)
        elif c == "=" and depth == 0:
            skipping = True
        elif not skipping:
            out.append(c)
    return " ".join("".join(out).split())


def make_override(decl: str) -> str:
    """Turn a pure-virtual declaration (without trailing `;`) into an override definition."""
    decl = re.sub(r"=\s*0$", "", decl).strip()
    decl = re.sub(r"^virtual\b", "", decl).strip()

    p_open = decl.index("(")
    before = decl[:p_open]
    name_match = re.search(r"([A-Za-z_~][A-Za-z0-9_]*)\s*$", before)
    if not name_match:
        raise ValueError(f"could not find method name in: {decl!r}")
    name = name_match.group(1)
    ret = before[: name_match.start()].strip()

    depth, p_close = 0, None
    for i in range(p_open, len(decl)):
        if decl[i] == "(":
            depth += 1
        elif decl[i] == ")":
            depth -= 1
            if depth == 0:
                p_close = i
                break
    if p_close is None:
        raise ValueError(f"unbalanced parens in: {decl!r}")

    params = strip_default_args(decl[p_open + 1 : p_close])
    qual = decl[p_close + 1 :].strip()

    body = "{}" if ret == "void" else "{ return {}; }"
    qual_str = f" {qual}" if qual else ""
    return f"{ret} {name}({params}){qual_str} override {body}"


def parse_pure_virtuals(header_text: str, class_name: str) -> list[str]:
    body = extract_class_body(strip_comments(header_text), class_name)
    overrides = []
    for decl, had_body in iter_member_decls(body):
        if had_body or "virtual" not in decl:
            continue
        if not re.search(r"=\s*0$", decl):
            continue
        overrides.append(make_override(decl))
    return overrides


def guard_macro(out_file: str) -> str:
    return "SERVO_ARKWEB_GENERATED_" + re.sub(r"[^A-Za-z0-9]", "_", out_file).upper() + "_"


def source_commit() -> str:
    info = VENDOR_DIR / "OHOS_SYNC_INFO.md"
    if info.exists():
        m = re.search(r"Commit\s*\|\s*`([0-9a-f]+)`", info.read_text())
        if m:
            return m.group(1)
    return "unknown"


def render(header: str, class_name: str, base_name: str, out_file: str) -> str:
    overrides = parse_pure_virtuals((VENDOR_DIR / header).read_text(), class_name)
    guard = guard_macro(out_file)
    lines = [
        "// @generated by ports/arkweb/tools/gen_stubs.py -- DO NOT EDIT.",
        f"// Source: vendored ohos_nweb/{header}, class OHOS::NWeb::{class_name}",
        f"// OHOS commit: {source_commit()}",
        f"// Overrides all {len(overrides)} pure virtuals with trivial defaults.",
        f"#ifndef {guard}",
        f"#define {guard}",
        "",
        f'#include "ohos_nweb/{header}"',
        "",
        "namespace OHOS::NWeb {",
        "",
        "// Concrete-izing base: subclass this and override only the methods the Servo",
        "// ArkWeb backend actually implements.",
        f"class {base_name} : public {class_name} {{",
        "public:",
    ]
    lines += [f"    {ov}" for ov in overrides]
    lines += [
        "};",
        "",
        "}  // namespace OHOS::NWeb",
        "",
        f"#endif  // {guard}",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify checked-in output is current")
    args = ap.parse_args()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    stale = []
    for header, class_name, base_name, out_file in TARGETS:
        content = render(header, class_name, base_name, out_file)
        n = content.count(" override {")
        path = OUT_DIR / out_file
        if args.check:
            current = path.read_text() if path.exists() else None
            status = "OK" if current == content else "STALE"
            if current != content:
                stale.append(out_file)
            print(f"  [{status}] {out_file}: {n} overrides ({class_name})")
        else:
            path.write_text(content)
            print(f"  wrote {out_file}: {n} overrides ({class_name})")

    if args.check and stale:
        print(f"\nERROR: {len(stale)} generated file(s) out of date: {', '.join(stale)}", file=sys.stderr)
        print("Run: python3 ports/arkweb/tools/gen_stubs.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
