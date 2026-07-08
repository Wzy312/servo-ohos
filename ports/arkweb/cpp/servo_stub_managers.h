#ifndef SERVO_ARKWEB_SERVO_STUB_MANAGERS_H
#define SERVO_ARKWEB_SERVO_STUB_MANAGERS_H

#include <memory>
#include <string>

#include "generated/servo_cookie_manager_stub_base.h"
#include "generated/servo_data_base_stub_base.h"
#include "generated/servo_download_manager_stub_base.h"
#include "generated/servo_preference_stub_base.h"
#include "generated/servo_web_storage_stub_base.h"

namespace OHOS::NWeb {

// web_delegate.cpp calls GetPreference()/GetCookieManager() under CHECK_NULL_VOID *before*
// installing the handler, so these must be non-null even though Servo does not back them yet.
// The stub bases already turn every pure virtual into a trivial default, so the concrete
// classes are empty.
// Sync cookie get/set/clear backed by Servo's site-data manager (via the cxx bridge). The async
// callback variants and per-cookie enumeration remain the stub base's no-ops for now.
class ServoCookieManager : public ServoCookieManagerStubBase {
public:
    std::string ReturnCookie(const std::string& url, bool& is_valid, bool incognito_mode) override;
    std::string ReturnCookieWithHttpOnly(const std::string& url, bool& is_valid,
                                         bool incognito_mode, bool includeHttpOnly) override;
    int SetCookie(const std::string& url, const std::string& value, bool incognito_mode) override;
    int SetCookieWithHttpOnly(const std::string& url, const std::string& value, bool incognito_mode,
                              bool includeHttpOnly) override;
    int SetCookieSync(const std::string& url, const std::string& value, bool incognitoMode,
                      bool includeHttpOnly) override;
};
class ServoPreference : public ServoPreferenceStubBase {};
class ServoWebStorage : public ServoWebStorageStubBase {};
class ServoDataBase : public ServoDataBaseStubBase {};
class ServoDownloadManager : public ServoDownloadManagerStubBase {};

// Process-wide singletons (leaked at process exit).
std::shared_ptr<NWebCookieManager> GetServoCookieManager();
std::shared_ptr<NWebPreference> GetServoPreference();
std::shared_ptr<NWebWebStorage> GetServoWebStorage();
std::shared_ptr<NWebDataBase> GetServoDataBase();
std::shared_ptr<NWebDownloadManager> GetServoDownloadManager();

}  // namespace OHOS::NWeb

#endif  // SERVO_ARKWEB_SERVO_STUB_MANAGERS_H
