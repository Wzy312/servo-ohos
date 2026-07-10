#include "servo_handler_proxy.h"

#include <cstdint>
#include <string>
#include <utility>

#include "arkweb/src/bridge.rs.h"
#include "ohos_nweb/nweb_access_request.h"
#include "ohos_nweb/nweb_console_log.h"
#include "ohos_nweb/nweb_file_selector_params.h"
#include "ohos_nweb/nweb_geolocation_callback_interface.h"
#include "ohos_nweb/nweb_js_dialog_result.h"
#include "ohos_nweb/nweb_js_http_auth_result.h"
#include "ohos_nweb/nweb_select_popup_menu.h"
#include "ohos_nweb/nweb_value_callback.h"

namespace servo::arkweb {

namespace {

// The NWebJSDialogResult handed to ACE for a JS dialog; ACE calls back on it when the user
// responds, which we route to the parked Servo SimpleDialog by id.
class ServoJsDialogResult : public OHOS::NWeb::NWebJSDialogResult {
public:
    explicit ServoJsDialogResult(std::uint64_t dialog_id) : dialog_id_(dialog_id) {}

    void Confirm() override { servo::arkweb::resolve_js_dialog(dialog_id_, true, std::string()); }
    void Confirm(const std::string& message) override {
        servo::arkweb::resolve_js_dialog(dialog_id_, true, message);
    }
    void Cancel() override { servo::arkweb::resolve_js_dialog(dialog_id_, false, std::string()); }

private:
    std::uint64_t dialog_id_;
};
// Concrete NWebConsoleLog carrying a servo console message across to OnConsoleLog. Servo provides
// only the level and text; the line number and source id are unknown (0 / empty).
class ServoConsoleLog : public OHOS::NWeb::NWebConsoleLog {
public:
    ServoConsoleLog(std::string message, NWebConsoleLogLevel level)
        : message_(std::move(message)), level_(level) {}

    int LineNumer() override { return 0; }
    std::string Log() override { return message_; }
    NWebConsoleLogLevel LogLevel() override { return level_; }
    std::string SourceId() override { return {}; }

private:
    std::string message_;
    NWebConsoleLogLevel level_;
};
}  // namespace

void NWebHandlerProxy::set_handler(std::shared_ptr<OHOS::NWeb::NWebHandler> handler) {
    std::lock_guard<std::mutex> lock(mutex_);
    handler_ = std::move(handler);
}

std::shared_ptr<OHOS::NWeb::NWebHandler> NWebHandlerProxy::handler() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return handler_;
}

void NWebHandlerProxy::on_load_started(const std::string& url) const {
    if (auto h = handler()) {
        h->OnPageLoadBegin(url);
    }
}

void NWebHandlerProxy::on_load_finished(const std::string& url, std::int32_t http_status) const {
    if (auto h = handler()) {
        h->OnPageLoadEnd(static_cast<int>(http_status), url);
    }
}

void NWebHandlerProxy::on_load_error(std::int32_t code, const std::string& desc,
                                     const std::string& url) const {
    if (auto h = handler()) {
        h->OnPageLoadError(static_cast<int>(code), desc, url);
    }
}

// Servo fires notify_url_changed on every URL change, including history
// traversal and history.pushState/replaceState, which do not go through a
// fresh page load (so OnPageLoadBegin/End would not fire). OnRefreshAccessedHistory
// is ArkWeb's navigation-committed callback (ArkTS onRefreshAccessedHistory), which
// carries the new URL and lets the embedder keep its URL bar in sync.
void NWebHandlerProxy::on_url_changed(const std::string& url) const {
    if (auto h = handler()) {
        h->OnRefreshAccessedHistory(url, false);
    }
}

void NWebHandlerProxy::on_title_changed(const std::string& title) const {
    if (auto h = handler()) {
        h->OnPageTitle(title);
    }
}

void NWebHandlerProxy::on_progress(std::int32_t progress) const {
    if (auto h = handler()) {
        h->OnLoadingProgress(static_cast<int>(progress));
    }
}

void NWebHandlerProxy::on_history_changed(bool /*can_back*/, bool /*can_fwd*/) const {}

void NWebHandlerProxy::on_console_message(std::int32_t level, const std::string& msg,
                                          std::int32_t /*line*/, const std::string& /*source*/) const {
    if (auto h = handler()) {
        auto log = std::make_shared<ServoConsoleLog>(
            msg, static_cast<OHOS::NWeb::NWebConsoleLog::NWebConsoleLogLevel>(level));
        h->OnConsoleLog(std::move(log));
    }
}

// Presentation is driven directly on the servo side (EGL swap); nothing to forward.
void NWebHandlerProxy::on_frame_ready() const {}

void NWebHandlerProxy::update_text_field_status(bool show_keyboard, bool attach_ime) const {
    if (auto h = handler()) {
        h->UpdateTextFieldStatus(show_keyboard, attach_ime);
    }
}

namespace {
using namespace OHOS::NWeb;

class ServoSelectMenuBound : public NWebSelectMenuBound {
public:
    ServoSelectMenuBound(int x, int y, int width, int height)
        : x_(x), y_(y), width_(width), height_(height) {}
    int GetX() override { return x_; }
    int GetY() override { return y_; }
    int GetWidth() override { return width_; }
    int GetHeight() override { return height_; }

private:
    int x_, y_, width_, height_;
};

// One `<option>` presented to ACE's dropdown menu.
class ServoSelectPopupMenuItem : public NWebSelectPopupMenuItem {
public:
    ServoSelectPopupMenuItem(std::string label, bool checked)
        : label_(std::move(label)), checked_(checked) {}
    SelectPopupMenuItemType GetType() override { return SP_OPTION; }
    std::string GetLabel() override { return label_; }
    uint32_t GetAction() override { return 0; }
    std::string GetToolTip() override { return {}; }
    bool GetIsChecked() override { return checked_; }
    bool GetIsEnabled() override { return true; }
    TextDirection GetTextDirection() override { return SP_LTR; }
    bool GetHasTextDirectionOverride() override { return false; }

private:
    std::string label_;
    bool checked_;
};

class ServoSelectPopupMenuParam : public NWebSelectPopupMenuParam {
public:
    ServoSelectPopupMenuParam(std::vector<std::shared_ptr<NWebSelectPopupMenuItem>> items, int selected,
                              bool multiple, std::shared_ptr<NWebSelectMenuBound> bound)
        : items_(std::move(items)), selected_(selected), multiple_(multiple), bound_(std::move(bound)) {}
    std::vector<std::shared_ptr<NWebSelectPopupMenuItem>> GetMenuItems() override { return items_; }
    // Item height (vp) and font size (fp) for the ACE-rendered rows; 0 leaves the labels invisible.
    int GetItemHeight() override { return 48; }
    int GetSelectedItem() override { return selected_; }
    double GetItemFontSize() override { return 16.0; }
    bool GetIsRightAligned() override { return false; }
    std::shared_ptr<NWebSelectMenuBound> GetSelectMenuBound() override { return bound_; }
    bool GetIsAllowMultipleSelection() override { return multiple_; }

private:
    std::vector<std::shared_ptr<NWebSelectPopupMenuItem>> items_;
    int selected_;
    bool multiple_;
    std::shared_ptr<NWebSelectMenuBound> bound_;
};

// The callback ACE invokes when the user picks an option (Continue) or dismisses the menu (Cancel).
class ServoSelectPopupMenuCallback : public NWebSelectPopupMenuCallback {
public:
    explicit ServoSelectPopupMenuCallback(std::uint64_t select_id) : select_id_(select_id) {}
    void Continue(const std::vector<int32_t>& indices) override {
        servo::arkweb::select_popup_continue(select_id_, indices);
    }
    void Cancel() override { servo::arkweb::select_popup_cancel(select_id_); }

private:
    std::uint64_t select_id_;
};
}  // namespace

bool NWebHandlerProxy::show_select_popup(std::uint64_t select_id, const std::string& labels,
                                         std::int32_t selected, bool multiple, std::int32_t x,
                                         std::int32_t y, std::int32_t width,
                                         std::int32_t height) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    std::vector<std::shared_ptr<OHOS::NWeb::NWebSelectPopupMenuItem>> items;
    std::size_t start = 0;
    for (int index = 0; start <= labels.size(); ++index) {
        std::size_t end = labels.find('\n', start);
        std::string label = labels.substr(start, end == std::string::npos ? std::string::npos : end - start);
        items.push_back(std::make_shared<ServoSelectPopupMenuItem>(std::move(label), index == selected));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    auto bound = std::make_shared<ServoSelectMenuBound>(x, y, width, height);
    auto param =
        std::make_shared<ServoSelectPopupMenuParam>(std::move(items), selected, multiple, std::move(bound));
    auto callback = std::make_shared<ServoSelectPopupMenuCallback>(select_id);
    h->OnSelectPopupMenu(param, callback);
    return true;
}

namespace {

// The <input type=file> parameters handed to ACE / the app's file-selector handler.
class ServoFileSelectorParams : public NWebFileSelectorParams {
public:
    ServoFileSelectorParams(std::vector<std::string> accept_types, bool multiple)
        : accept_types_(std::move(accept_types)), multiple_(multiple) {}
    const std::string Title() override { return {}; }
    FileSelectorMode Mode() override {
        return multiple_ ? FILE_OPEN_MULTIPLE_MODE : FILE_OPEN_MODE;
    }
    const std::string DefaultFilename() override { return {}; }
    const AcceptTypeList AcceptType() override { return accept_types_; }
    bool IsCapture() override { return false; }

private:
    std::vector<std::string> accept_types_;
    bool multiple_;
};

// The value callback ACE / the app invokes with the chosen file paths (an empty list means the
// selection was cancelled), routed back to the parked Servo FilePicker by id.
class ServoFileSelectorCallback : public NWebStringVectorValueCallback {
public:
    explicit ServoFileSelectorCallback(std::uint64_t picker_id) : picker_id_(picker_id) {}
    void OnReceiveValue(const std::vector<std::string>& value) override {
        if (value.empty()) {
            servo::arkweb::file_picker_cancel(picker_id_);
        } else {
            servo::arkweb::file_picker_continue(picker_id_, value);
        }
    }

private:
    std::uint64_t picker_id_;
};
}  // namespace

bool NWebHandlerProxy::show_file_picker(std::uint64_t picker_id, const std::string& accept_types,
                                        bool multiple) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    std::vector<std::string> types;
    if (!accept_types.empty()) {
        std::size_t start = 0;
        while (start <= accept_types.size()) {
            std::size_t end = accept_types.find('\n', start);
            types.push_back(accept_types.substr(
                start, end == std::string::npos ? std::string::npos : end - start));
            if (end == std::string::npos) {
                break;
            }
            start = end + 1;
        }
    }
    auto params = std::make_shared<ServoFileSelectorParams>(std::move(types), multiple);
    auto callback = std::make_shared<ServoFileSelectorCallback>(picker_id);
    return h->OnFileSelectorShow(callback, params);
}

bool NWebHandlerProxy::show_js_dialog(std::uint64_t dialog_id, std::int32_t kind,
                                      const std::string& message,
                                      const std::string& default_value) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    auto result = std::make_shared<ServoJsDialogResult>(dialog_id);
    // Servo does not surface the triggering page URL here; ACE tolerates an empty url.
    const std::string url;
    switch (kind) {
        case 0:
            return h->OnAlertDialogByJS(url, message, result);
        case 1:
            return h->OnConfirmDialogByJS(url, message, result);
        case 2:
            return h->OnPromptDialogByJS(url, message, default_value, result);
        default:
            return false;
    }
}

namespace {
// The callback ACE / the app invokes with the geolocation decision, routed back to the parked
// Servo PermissionRequest by id. `retain`/`incognito` have no Servo equivalent and are dropped.
class ServoGeolocationCallback : public NWebGeolocationCallbackInterface {
public:
    explicit ServoGeolocationCallback(std::uint64_t request_id) : request_id_(request_id) {}
    void GeolocationCallbackInvoke(const std::string& /*origin*/, bool allow, bool /*retain*/,
                                   bool /*incognito*/) override {
        servo::arkweb::resolve_permission(request_id_, allow);
    }

private:
    std::uint64_t request_id_;
};

// The access request handed to ACE / the app for non-geolocation permission prompts
// (camera / microphone). Agree/Refuse route back to the parked Servo PermissionRequest.
class ServoAccessRequest : public NWebAccessRequest {
public:
    ServoAccessRequest(std::uint64_t request_id, std::string origin, int resources)
        : request_id_(request_id), origin_(std::move(origin)), resources_(resources) {}
    std::string Origin() override { return origin_; }
    int ResourceAcessId() override { return resources_; }
    void Agree(int /*resourceId*/) override { servo::arkweb::resolve_permission(request_id_, true); }
    void Refuse() override { servo::arkweb::resolve_permission(request_id_, false); }

private:
    std::uint64_t request_id_;
    std::string origin_;
    int resources_;
};

// The result object ACE / the app invokes with HTTP-auth credentials (or cancellation), routed
// back to the parked Servo AuthenticationRequest by id.
class ServoHttpAuthResult : public NWebJSHttpAuthResult {
public:
    explicit ServoHttpAuthResult(std::uint64_t request_id) : request_id_(request_id) {}
    bool Confirm(const std::string& userName, const std::string& pwd) override {
        servo::arkweb::resolve_http_auth(request_id_, true, userName, pwd);
        return true;
    }
    void Cancel() override { servo::arkweb::resolve_http_auth(request_id_, false, "", ""); }
    bool IsHttpAuthInfoSaved() override { return false; }

private:
    std::uint64_t request_id_;
};
}  // namespace

bool NWebHandlerProxy::show_geolocation_permission(std::uint64_t request_id,
                                                   const std::string& origin) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    h->OnGeolocationShow(origin, std::make_shared<ServoGeolocationCallback>(request_id));
    return true;
}

bool NWebHandlerProxy::show_permission_request(std::uint64_t request_id, const std::string& origin,
                                               std::int32_t resources) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    h->OnPermissionRequest(std::make_shared<ServoAccessRequest>(request_id, origin, resources));
    return true;
}

bool NWebHandlerProxy::show_http_auth_request(std::uint64_t request_id, const std::string& host,
                                              const std::string& realm) const {
    auto h = handler();
    if (!h) {
        return false;
    }
    return h->OnHttpAuthRequestByJS(std::make_shared<ServoHttpAuthResult>(request_id), host, realm);
}

}  // namespace servo::arkweb
