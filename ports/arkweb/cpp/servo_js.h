#pragma once

#include <cstdint>
#include <memory>
#include <string>

#include "ohos_nweb/nweb_value_callback.h"

namespace servo::arkweb {

// Register an ArkWeb JS-result callback, returning an id to hand to the Rust evaluator. The
// matching deliver_js_result later invokes and removes it.
std::uint64_t register_js_callback(std::shared_ptr<OHOS::NWeb::NWebMessageValueCallback> callback);

// Deliver a JavaScript evaluation result (rendered as a string) to the registered callback, from
// the servo thread once evaluation completes. No-op if the id is unknown.
void deliver_js_result(std::uint64_t eval_id, const std::string& value, bool success);

}  // namespace servo::arkweb
