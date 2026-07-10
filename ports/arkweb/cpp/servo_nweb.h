#ifndef SERVO_ARKWEB_SERVO_NWEB_H
#define SERVO_ARKWEB_SERVO_NWEB_H

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include "generated/servo_nweb_stub_base.h"
#include "servo_handler_proxy.h"

namespace OHOS::NWeb {

// One Servo-backed webview. Declared inside OHOS::NWeb so the overridden signatures match the
// vendored (unqualified) NWeb declarations verbatim. MVP methods forward to the cxx bridge
// keyed by the servo-side webview id; everything else falls through to the stub base.
class ServoNWeb : public ServoNWebStubBase {
public:
    ServoNWeb(uint32_t id, std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy);

    void Resize(uint32_t width, uint32_t height, bool isKeyboard) override;
    void OnPause() override;
    void OnContinue() override;
    void OnOccluded() override;
    void OnUnoccluded() override;
    void OnDestroy() override;
    void OnFocus(const FocusReason& focusReason) override;
    void OnBlur(const BlurReason& blurReason) override;
    void OnTouchPress(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchRelease(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchMove(int32_t id, double x, double y, bool fromOverlay) override;
    void OnTouchMove(const std::vector<std::shared_ptr<NWebTouchPointInfo>>& touch_point_infos,
                     bool fromOverlay) override;
    void OnTouchCancel() override;
    bool SendKeyEvent(int32_t keyCode, int32_t keyAction) override;
    bool SendKeyboardEvent(const std::shared_ptr<NWebKeyboardEvent>& keyboardEvent) override;
    bool NeedSoftKeyboard() override;
    int Load(const std::string& url) override;
    bool IsNavigatebackwardAllowed() override;
    bool IsNavigateForwardAllowed() override;
    bool CanNavigateBackOrForward(int numSteps) override;
    void NavigateBack() override;
    void NavigateForward() override;
    void NavigateBackOrForward(int step) override;
    void Reload() override;
    int Zoom(float zoomFactor) override;
    void ExecuteJavaScript(const std::string& code) override;
    void ExecuteJavaScript(const std::string& code,
                           std::shared_ptr<NWebMessageValueCallback> callback,
                           bool extention) override;
    unsigned int GetWebId() override;
    void SetNWebHandler(std::shared_ptr<NWebHandler> handler) override;
    std::string GetUrl() override;
    std::string Title() override;
    int PageLoadProgress() override;
    const std::string GetOriginalUrl() override;
    void ScrollTo(float x, float y) override;
    void ScrollBy(float delta_x, float delta_y) override;
    void ScrollByRefScreen(float delta_x, float delta_y, float vx, float vy) override;
    void PageUp(bool top) override;
    void PageDown(bool bottom) override;
    void PauseAllMedia() override;
    void ResumeAllMedia() override;
    void StopAllMedia() override;
    int GetMediaPlaybackState() override;
    void PutBackgroundColor(int color) override;
    std::shared_ptr<NWebPreference> GetPreference() override;
    void SetNWebJavaScriptResultCallBack(
        std::shared_ptr<NWebJavaScriptResultCallBack> callback) override;
    void RegisterNativeValideCallback(const char* webName,
                                      const NativeArkWebOnValidCallback callback) override;
    void RegisterNativeDestroyCallback(const char* webName,
                                       const NativeArkWebOnDestroyCallback callback) override;
    std::shared_ptr<NWebDragData> GetOrCreateDragData() override;
    void PrecompileJavaScript(const std::string& url, const std::string& script,
                              std::shared_ptr<CacheOptions>& cacheOptions,
                              std::shared_ptr<NWebMessageValueCallback> callback) override;

private:
    // OnPause/OnContinue (window hide, app background) and OnOccluded/OnUnoccluded (covered by
    // another window) are independent signals; throttle while either says invisible.
    void UpdateThrottled();

    uint32_t id_;
    std::shared_ptr<servo::arkweb::NWebHandlerProxy> proxy_;
    int background_color_ = 0;
    bool paused_ = false;
    bool occluded_ = false;
    // Sink for injected-JS-proxy calls; set unconditionally by every WebviewController attach.
    // Only invoked once a JS-proxy implementation exists (not wired yet); stored so it is ready.
    std::shared_ptr<NWebJavaScriptResultCallBack> js_result_callback_;
    // Native ArkWeb C API (`OH_NativeArkWeb_*`) destroy callback, keyed by web tag; the valid
    // callback is fired directly on registration (the instance is live by then).
    std::string native_destroy_web_name_;
    NativeArkWebOnDestroyCallback native_destroy_callback_ = nullptr;
    std::shared_ptr<NWebDragData> drag_data_;
};

}  // namespace OHOS::NWeb

#endif  // SERVO_ARKWEB_SERVO_NWEB_H
