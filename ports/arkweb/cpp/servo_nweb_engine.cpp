#include "servo_nweb_engine.h"

#include <dlfcn.h>

#include <memory>
#include <utility>

#include "arkweb/src/bridge.rs.h"
#include "servo_handler_proxy.h"

namespace OHOS::NWeb {

namespace {
// CreateNativeWindowFromSurface is an inner OHOS API (graphic_surface .../surface/window.h),
// not exposed by the NDK stub lib, so it is resolved at runtime from the real
// libnative_window.so already loaded in the app process.
using CreateNativeWindowFromSurfaceFn = void* (*)(void*);
}  // namespace

std::shared_ptr<NWeb> ServoNWebEngine::CreateNWeb(std::shared_ptr<NWebCreateInfo> create_info) {
    if (!create_info) {
        return nullptr;
    }

    // The producer surface is the address of a by-value stack parameter in
    // NWebSurfaceAdapter::GetCreateInfo and dangles once CreateNWeb returns, so it must be
    // consumed here, before anything else.
    void* producer_surface = create_info->GetProducerSurface();
    if (create_info->GetEnhanceSurfaceInfo() != nullptr) {
        // Enhance-surface mode is unsupported in the MVP.
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

    auto proxy = std::make_shared<servo::arkweb::NWebHandlerProxy>();
    uint32_t id = servo::embedder::create_webview(reinterpret_cast<size_t>(native_window), width,
                                                  height, proxy);

    auto nweb = std::make_shared<ServoNWeb>(id, proxy);
    {
        std::lock_guard<std::mutex> lock(mutex_);
        nwebs_[id] = nweb;
    }
    return nweb;
}

std::shared_ptr<NWeb> ServoNWebEngine::GetNWeb(int32_t nweb_id) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = nwebs_.find(static_cast<uint32_t>(nweb_id));
    return it == nwebs_.end() ? nullptr : it->second;
}

void ServoNWebEngine::InitializeWebEngine(std::shared_ptr<NWebEngineInitArgs> /*init_args*/) {
    servo::embedder::InitOptions options{};
    // TODO(arkweb): parse --user-data-dir / --lang from init_args->GetArgsToAdd().
    servo::embedder::initialize(std::move(options));
}

void ServoNWebEngine::LibraryLoaded(std::shared_ptr<NWebEngineInitArgs> /*init_args*/, bool /*lazy*/) {}

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
