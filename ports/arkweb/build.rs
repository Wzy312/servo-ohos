fn main() {
    // The ArkWeb port only produces a functional library for OpenHarmony targets.
    // On any other target (e.g. host `./mach check`) this crate is an empty library,
    // so skip all C++/cxx bridge compilation.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("ohos") {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let prelude = format!("{manifest_dir}/cpp/arkweb_prelude.h");

    let cpp_files = [
        "cpp/entry.cpp",
        "cpp/servo_nweb_engine.cpp",
        "cpp/servo_nweb.cpp",
        "cpp/servo_handler_proxy.cpp",
        "cpp/servo_stub_managers.cpp",
        "cpp/servo_native_window.cpp",
        "cpp/servo_js.cpp",
    ];

    cxx_build::bridge("src/bridge.rs")
        .files(cpp_files)
        .include("vendor")
        .include("cpp")
        .std("c++17")
        // Force-include standard headers the vendored OHOS headers assume (see prelude).
        .flag("-include")
        .flag(&prelude)
        // nweb_drag_data.h sets an `enum class : unsigned char` enumerator to UINT_MAX; the
        // wrapped value is never used but the narrowing would otherwise be a hard error.
        .flag_if_supported("-Wno-c++11-narrowing")
        .flag_if_supported("-Wno-narrowing")
        .compile("servo_arkweb_cpp");

    println!("cargo:rerun-if-changed=src/bridge.rs");
    for file in cpp_files {
        println!("cargo:rerun-if-changed={file}");
    }
    println!("cargo:rerun-if-changed=cpp");
    println!("cargo:rerun-if-changed=vendor/ohos_nweb");

    // libservo_arkweb.so is dlopen'd by the patched NWeb helper, which only resolves the two
    // factory / ABI symbols (in lib.rs) via dlsym. Everything else should stay out of the dynamic
    // symbol table. Two link args cooperate to that end; neither is sufficient alone:
    //
    //   * `--exclude-libs,ALL` gives local visibility to every symbol pulled in from a static
    //     archive -- all Rust rlibs (std, the aws-lc crypto provider, ICU4X, encoding_rs, brotli,
    //     the cxx runtime, ...) and the linked C++ static lib. This is what actually strips the
    //     hundreds of third-party `#[no_mangle]` symbols M2 flagged. It cannot be done with a
    //     version script alone: rustc auto-generates its own `--version-script` for every cdylib
    //     that lists all reachable exported symbols as `global`, and lld unions version scripts,
    //     so a user `local: *` can never demote a symbol rustc already globalized.
    //   * the version script then declares the intended public ABI (the two factory symbols). The
    //     two `#[no_mangle]` wrappers live in the root crate object (not an archive), so they
    //     survive `--exclude-libs`; the version script documents that they -- and only they -- are
    //     the supported entry points. Mirrors the servoshell OpenHarmony port's `.ver` convention.
    //
    // The cxx `extern "Rust"` thunks in bridge.rs are also root-crate `#[no_mangle]` symbols, so
    // they remain exported too; fully hiding them would mean moving the bridge into a separate
    // rlib so `--exclude-libs` covers it.
    let version_script = format!("{manifest_dir}/libservo_arkweb.ver");
    assert!(
        std::path::Path::new(&version_script).exists(),
        "Expected version script to exist at path `{version_script}`"
    );
    println!("cargo:rerun-if-changed={version_script}");
    // Using `rustc-link-arg-cdylib` causes a false-positive warning:
    // https://github.com/rust-lang/cargo/issues/16487
    // We work around this by using the unconditional link-arg, which is fine since this crate is
    // always built as a cdylib.
    println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
    println!("cargo:rustc-link-arg=-Wl,--version-script={version_script}");
}
