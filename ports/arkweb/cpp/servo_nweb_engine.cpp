#include "servo_nweb_engine.h"

#include <dlfcn.h>
#include <hilog/log.h>

#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <utility>

#include "arkweb/src/bridge.rs.h"
#include "servo_handler_proxy.h"

namespace OHOS::NWeb {

namespace {
// CreateNativeWindowFromSurface is an inner OHOS API (graphic_surface .../surface/window.h),
// not exposed by the NDK stub lib, so it is resolved at runtime from the real
// libnative_window.so already loaded in the app process.
using CreateNativeWindowFromSurfaceFn = void* (*)(void*);

#define SERVO_LOGI(...) OH_LOG_Print(LOG_APP, LOG_INFO, 0xE0C3, "ServoArkWeb", __VA_ARGS__)

servo::embedder::InitOptions ParseInitOptions(const std::shared_ptr<NWebEngineInitArgs>& init_args) {
    servo::embedder::InitOptions options{};
    if (init_args) {
        constexpr std::string_view kUserDataDir = "--user-data-dir=";
        constexpr std::string_view kLang = "--lang=";
        for (const std::string& arg : init_args->GetArgsToAdd()) {
            if (arg.rfind(kUserDataDir, 0) == 0) {
                options.user_data_dir = arg.substr(kUserDataDir.size());
            } else if (arg.rfind(kLang, 0) == 0) {
                options.lang = arg.substr(kLang.size());
            }
        }
    }
    return options;
}
}  // namespace

std::shared_ptr<NWeb> ServoNWebEngine::CreateNWeb(std::shared_ptr<NWebCreateInfo> create_info) {
    if (!create_info) {
        SERVO_LOGI("CreateNWeb: null create_info");
        return nullptr;
    }

    // The producer surface is the address of a by-value stack parameter in
    // NWebSurfaceAdapter::GetCreateInfo and dangles once CreateNWeb returns, so it must be
    // consumed here, before anything else.
    void* producer_surface = create_info->GetProducerSurface();
    void* enhance_surface = create_info->GetEnhanceSurfaceInfo();
    SERVO_LOGI("CreateNWeb: enter w=%{public}u h=%{public}u producer=%{public}llx enhance=%{public}llx",
        create_info->GetWidth(), create_info->GetHeight(),
        static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(producer_surface)),
        static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(enhance_surface)));
    if (enhance_surface != nullptr) {
        // Enhance-surface mode is unsupported in the MVP.
        SERVO_LOGI("CreateNWeb: enhance-surface mode unsupported (MVP), returning null");
        return nullptr;
    }

    void* native_window = nullptr;
    if (producer_surface != nullptr) {
        auto create_native_window = reinterpret_cast<CreateNativeWindowFromSurfaceFn>(
            dlsym(RTLD_DEFAULT, "CreateNativeWindowFromSurface"));
        if (create_native_window != nullptr) {
            native_window = create_native_window(producer_surface);
        }
    }

    uint32_t width = create_info->GetWidth();
    uint32_t height = create_info->GetHeight();

    SERVO_LOGI("CreateNWeb: native_window=%{public}llx, calling create_webview %{public}ux%{public}u",
        static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(native_window)), width, height);
    auto proxy = std::make_shared<servo::arkweb::NWebHandlerProxy>();
    uint32_t id = servo::embedder::create_webview(reinterpret_cast<size_t>(native_window), width,
                                                  height, proxy);
    SERVO_LOGI("CreateNWeb: create_webview returned id=%{public}u", id);

    auto nweb = std::make_shared<ServoNWeb>(id, proxy);
    {
        std::lock_guard<std::mutex> lock(mutex_);
        // Prune entries whose web component has been destroyed.
        for (auto it = nwebs_.begin(); it != nwebs_.end();) {
            it = it->second.expired() ? nwebs_.erase(it) : std::next(it);
        }
        nwebs_[id] = nweb;
    }
    return nweb;
}

std::shared_ptr<NWeb> ServoNWebEngine::GetNWeb(int32_t nweb_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = nwebs_.find(static_cast<uint32_t>(nweb_id));
    return it == nwebs_.end() ? nullptr : it->second.lock();
}

// The Servo thread is started here rather than only in InitializeWebEngine: the OHOS lifecycle
// does not guarantee InitializeWebEngine runs before CreateNWeb (NWebHelper::CreateNWeb only
// checks that the engine object exists), but LibraryLoaded is always called from GetWebEngine,
// before any web component creates an NWeb. `initialize` is idempotent, so a later
// InitializeWebEngine call is a no-op.
void ServoNWebEngine::LibraryLoaded(std::shared_ptr<NWebEngineInitArgs> init_args, bool /*lazy*/) {
    servo::embedder::initialize(ParseInitOptions(init_args));
}

void ServoNWebEngine::InitializeWebEngine(std::shared_ptr<NWebEngineInitArgs> init_args) {
    servo::embedder::initialize(ParseInitOptions(init_args));
}

void ServoNWebEngine::SetWebTag(int32_t /*nweb_id*/, const char* /*web_tag*/) {}

std::shared_ptr<NWebCookieManager> ServoNWebEngine::GetCookieManager() {
    return GetServoCookieManager();
}

std::shared_ptr<NWebDataBase> ServoNWebEngine::GetDataBase() {
    return GetServoDataBase();
}

std::shared_ptr<NWebWebStorage> ServoNWebEngine::GetWebStorage() {
    return GetServoWebStorage();
}

std::shared_ptr<NWebDownloadManager> ServoNWebEngine::GetDownloadManager() {
    return GetServoDownloadManager();
}

}  // namespace OHOS::NWeb
