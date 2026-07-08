#include "servo_stub_managers.h"

#include "arkweb/src/bridge.rs.h"

namespace OHOS::NWeb {

std::string ServoCookieManager::ReturnCookie(const std::string& url, bool& is_valid,
                                             bool /*incognito_mode*/) {
    return ReturnCookieWithHttpOnly(url, is_valid, false, false);
}

std::string ServoCookieManager::ReturnCookieWithHttpOnly(const std::string& url, bool& is_valid,
                                                         bool /*incognito_mode*/,
                                                         bool includeHttpOnly) {
    std::string cookie(servo::embedder::cookie_get(url, includeHttpOnly));
    is_valid = !cookie.empty();
    return cookie;
}

int ServoCookieManager::SetCookie(const std::string& url, const std::string& value,
                                  bool /*incognito_mode*/) {
    return servo::embedder::cookie_set(url, value) ? 0 : -1;
}

int ServoCookieManager::SetCookieWithHttpOnly(const std::string& url, const std::string& value,
                                              bool /*incognito_mode*/, bool /*includeHttpOnly*/) {
    return servo::embedder::cookie_set(url, value) ? 0 : -1;
}

int ServoCookieManager::SetCookieSync(const std::string& url, const std::string& value,
                                      bool /*incognitoMode*/, bool /*includeHttpOnly*/) {
    return servo::embedder::cookie_set(url, value) ? 0 : -1;
}

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
