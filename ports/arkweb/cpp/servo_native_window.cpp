#include "servo_native_window.h"

#include <dlfcn.h>
#include <hilog/log.h>
#include <native_window/external_window.h>

#include <cstdint>

namespace servo::arkweb {

namespace {
#define SERVO_LOGI(...) OH_LOG_Print(LOG_APP, LOG_INFO, 0xE0C3, "ServoArkWeb", __VA_ARGS__)
#define SERVO_LOGE(...) OH_LOG_Print(LOG_APP, LOG_ERROR, 0xE0C3, "ServoArkWeb", __VA_ARGS__)

// Variadic, matching the public NDK declaration. Resolved at runtime from libnative_window.so
// (already loaded in the app process), consistent with how CreateNativeWindowFromSurface is used.
using HandleOptFn = int32_t (*)(OHNativeWindow*, int, ...);
using DestroyWindowFn = void (*)(OHNativeWindow*);
}  // namespace

void set_native_window_buffer_geometry(std::size_t window, std::uint32_t width, std::uint32_t height) {
    auto* native_window = reinterpret_cast<OHNativeWindow*>(window);
    if (native_window == nullptr || width == 0 || height == 0) {
        return;
    }
    auto handle_opt =
        reinterpret_cast<HandleOptFn>(dlsym(RTLD_DEFAULT, "OH_NativeWindow_NativeWindowHandleOpt"));
    if (handle_opt == nullptr) {
        SERVO_LOGE("OH_NativeWindow_NativeWindowHandleOpt not found: %{public}s", dlerror());
        return;
    }
    int32_t ret = handle_opt(native_window, SET_BUFFER_GEOMETRY, static_cast<int32_t>(width),
                             static_cast<int32_t>(height));
    SERVO_LOGI("SET_BUFFER_GEOMETRY %{public}ux%{public}u ret=%{public}d", width, height, ret);
}

void destroy_native_window(std::size_t window) {
    auto* native_window = reinterpret_cast<OHNativeWindow*>(window);
    if (native_window == nullptr) {
        return;
    }
    auto destroy =
        reinterpret_cast<DestroyWindowFn>(dlsym(RTLD_DEFAULT, "OH_NativeWindow_DestroyNativeWindow"));
    if (destroy == nullptr) {
        SERVO_LOGE("OH_NativeWindow_DestroyNativeWindow not found: %{public}s", dlerror());
        return;
    }
    destroy(native_window);
}

}  // namespace servo::arkweb
