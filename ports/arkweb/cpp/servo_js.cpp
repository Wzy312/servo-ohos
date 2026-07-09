#include "servo_js.h"

#include <mutex>
#include <unordered_map>
#include <utility>

#include "ohos_nweb/nweb_web_message.h"

namespace servo::arkweb {

namespace {
std::mutex g_mutex;
std::unordered_map<std::uint64_t, std::shared_ptr<OHOS::NWeb::NWebMessageValueCallback>> g_callbacks;
std::uint64_t g_next_id = 1;
}  // namespace

std::uint64_t register_js_callback(std::shared_ptr<OHOS::NWeb::NWebMessageValueCallback> callback) {
    std::lock_guard<std::mutex> lock(g_mutex);
    std::uint64_t id = g_next_id++;
    g_callbacks[id] = std::move(callback);
    return id;
}

void deliver_js_result(std::uint64_t eval_id, const std::string& value, bool /*success*/) {
    std::shared_ptr<OHOS::NWeb::NWebMessageValueCallback> callback;
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        auto it = g_callbacks.find(eval_id);
        if (it != g_callbacks.end()) {
            callback = std::move(it->second);
            g_callbacks.erase(it);
        }
    }
    if (callback) {
        // runJavaScript (non-Ext) expects a string result; wrap it in a STRING NWebMessage.
        auto message =
            std::make_shared<OHOS::NWeb::NWebMessage>(OHOS::NWeb::NWebValue::Type::STRING);
        message->SetString(value);
        callback->OnReceiveValue(std::move(message));
    }
}

}  // namespace servo::arkweb
