#ifndef SERVO_ARKWEB_SERVO_NWEB_ENGINE_H
#define SERVO_ARKWEB_SERVO_NWEB_ENGINE_H

#include <cstdint>
#include <memory>
#include <mutex>
#include <unordered_map>

#include "generated/servo_nweb_engine_stub_base.h"
#include "servo_nweb.h"
#include "servo_stub_managers.h"

namespace OHOS::NWeb {

// The Servo ArkWeb engine. A single instance is created by the factory in entry.cpp and lives
// for the process lifetime. Declared inside OHOS::NWeb so overridden signatures match the
// vendored NWebEngine declarations verbatim.
class ServoNWebEngine : public ServoNWebEngineStubBase {
public:
    std::shared_ptr<NWeb> CreateNWeb(std::shared_ptr<NWebCreateInfo> create_info) override;
    std::shared_ptr<NWeb> GetNWeb(int32_t nweb_id) override;
    void InitializeWebEngine(std::shared_ptr<NWebEngineInitArgs> init_args) override;
    void LibraryLoaded(std::shared_ptr<NWebEngineInitArgs> init_args, bool lazy) override;
    void SetWebTag(int32_t nweb_id, const char* web_tag) override;

    std::shared_ptr<NWebCookieManager> GetCookieManager() override;
    std::shared_ptr<NWebDataBase> GetDataBase() override;
    std::shared_ptr<NWebWebStorage> GetWebStorage() override;
    std::shared_ptr<NWebDownloadManager> GetDownloadManager() override;

private:
    std::mutex mutex_;
    // Weak, so a destroyed web component's NWeb (and the ACE handler its proxy pins) is released
    // once ACE drops its shared_ptr, rather than leaked here on every open/close. GetNWeb locks it;
    // expired entries are pruned on the next CreateNWeb.
    std::unordered_map<uint32_t, std::weak_ptr<NWeb>> nwebs_;
};

}  // namespace OHOS::NWeb

#endif  // SERVO_ARKWEB_SERVO_NWEB_ENGINE_H
