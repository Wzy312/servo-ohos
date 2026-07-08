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
}
