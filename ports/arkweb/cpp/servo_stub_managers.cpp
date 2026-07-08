#include "servo_stub_managers.h"

namespace OHOS::NWeb {

std::shared_ptr<NWebCookieManager> GetServoCookieManager() {
    static auto instance = std::make_shared<ServoCookieManager>();
    return instance;
}

std::shared_ptr<NWebPreference> GetServoPreference() {
    static auto instance = std::make_shared<ServoPreference>();
    return instance;
}

std::shared_ptr<NWebWebStorage> GetServoWebStorage() {
    static auto instance = std::make_shared<ServoWebStorage>();
    return instance;
}

std::shared_ptr<NWebDataBase> GetServoDataBase() {
    static auto instance = std::make_shared<ServoDataBase>();
    return instance;
}

std::shared_ptr<NWebDownloadManager> GetServoDownloadManager() {
    static auto instance = std::make_shared<ServoDownloadManager>();
    return instance;
}

}  // namespace OHOS::NWeb
