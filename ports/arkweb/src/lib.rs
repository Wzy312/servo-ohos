//! Servo as an alternative ArkWeb (OpenHarmony) webview backend.
//!
//! This crate builds a single self-contained `cdylib` (`libservo_arkweb.so`) that
//! implements the OHOS `NWebEngine` / `NWeb` C++ interfaces on top of libservo, so that
//! Servo can be selected at runtime as an alternative to the Chromium-based ArkWeb engine
//! behind the same ArkTS `Web` / `WebviewController` API.
//!
//! The port is only meaningful on OpenHarmony targets: everything that touches `servo` or the
//! C++ NWeb bridge is gated on `target_env = "ohos"`, so on any other target (e.g. host
//! `./mach check`) the crate compiles to an empty library. The exception is [`convert`], a
//! dependency-free module of pure conversion helpers kept host-compilable so it can be unit-tested.

// Pure helpers with no `servo`/OHOS coupling; compiled where they are used (ohos) or tested (host).
#[cfg(any(target_env = "ohos", test))]
mod convert;

#[cfg(target_env = "ohos")]
mod bridge;
#[cfg(target_env = "ohos")]
mod resources;
#[cfg(target_env = "ohos")]
mod runtime;

// The factory / ABI entry points dlsym'd by the patched nweb_helper.cpp SERVO branch. A Rust
// cdylib localizes symbols that come from linked C++ static archives, so the C++ factory cannot
// be exported directly. Instead these thin `#[unsafe(no_mangle)] pub extern "C"` wrappers (which
// rustc always exports) forward to the C++ `_impl` halves in entry.cpp; referencing the impls
// also forces `entry.o` to be linked in.
#[cfg(target_env = "ohos")]
use core::ffi::{c_int, c_void};

#[cfg(target_env = "ohos")]
unsafe extern "C" {
    fn servo_arkweb_create_nweb_engine_impl() -> *mut c_void;
    fn servo_arkweb_abi_version_impl() -> c_int;
}

#[cfg(target_env = "ohos")]
#[unsafe(no_mangle)]
pub extern "C" fn servo_arkweb_create_nweb_engine() -> *mut c_void {
    unsafe { servo_arkweb_create_nweb_engine_impl() }
}

#[cfg(target_env = "ohos")]
#[unsafe(no_mangle)]
pub extern "C" fn servo_arkweb_abi_version() -> c_int {
    unsafe { servo_arkweb_abi_version_impl() }
}
