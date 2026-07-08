#include "servo_nweb_engine.h"

// Implementation halves of the factory / ABI symbols. They are re-exported under their public
// names by thin `#[no_mangle]` Rust wrappers in lib.rs, because a Rust cdylib localizes symbols
// that come from linked C++ static archives (only Rust's own exported symbols make the dynamic
// table). These `_impl` symbols therefore only need C linkage, not default visibility.
extern "C" {

// Returns the process-wide Servo NWebEngine (leaked singleton). The loader wraps it in a
// no-op-deleter shared_ptr.
OHOS::NWeb::NWebEngine* servo_arkweb_create_nweb_engine_impl() {
    static auto* engine = new OHOS::NWeb::ServoNWebEngine();
    return engine;
}

// Cheap skew insurance: the loader can compare this against its own expected value before
// trusting the vtable layout of the returned engine.
int servo_arkweb_abi_version_impl() {
    return 1;
}

}  // extern "C"
