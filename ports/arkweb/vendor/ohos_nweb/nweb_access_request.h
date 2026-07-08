/*
 * Copyright (c) 2022 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#ifndef NWEB_ACCESS_REQUEST_H
#define NWEB_ACCESS_REQUEST_H

#include <memory>
#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebAccessRequest {
public:
    NWebAccessRequest() = default;

    virtual ~NWebAccessRequest() = default;

    enum Resources {
        GEOLOCATION = 1 << 0,
        VIDEO_CAPTURE = 1 << 1,
        AUDIO_CAPTURE = 1 << 2,
        PROTECTED_MEDIA_ID = 1 << 3,
        MIDI_SYSEX = 1 << 4,
        CLIPBOARD_READ_WRITE = 1 << 5,
        CLIPBOARD_SANITIZED_WRITE = 1 << 6,
        SENSORS = 1 << 7,
        NOTIFICATION = 1 << 8,
    };

    /**
     * Get the origin of the web page which is trying to access the resource.
     *
     * @return the origin of the web page which is trying to access the resource.
     */
    virtual std::string Origin() = 0;

    /**
     * Get the resource id the web page is trying to access.
     *
     * @return the resource id the web page is trying to access.
     */
    virtual int ResourceAcessId() = 0;

    /**
     * Agree the origin to access the given resources.
     * The granted access is only valid for this WebView.
     *
     * @param resourceId id of the resource agreed to be accessed by origin. It
     * must be equal to requested resource id returned by {@link
     * #GetResourceAcessId()}.
     */
    virtual void Agree(int resourceId) = 0;

    /**
     * Refuse the request.
     */
    virtual void Refuse() = 0;
};

class NWebScreenCaptureConfig {
public:
    virtual ~NWebScreenCaptureConfig() = default;

    virtual int32_t GetMode() = 0;
    virtual int32_t GetSourceId() = 0;
};

class OHOS_NWEB_EXPORT NWebScreenCaptureAccessRequest {
public:
    NWebScreenCaptureAccessRequest() = default;

    virtual ~NWebScreenCaptureAccessRequest() = default;

    /**
     * Get the origin of the web page which is trying to access the screen capture resource.
     *
     * @return the origin of the web page which is trying to access the resource.
     */
    virtual std::string Origin() = 0;

    /**
     * Agree the origin to access the given resources.
     * The granted access is only valid for this WebView.
     *
     * @param config screen capture config.
     */
    virtual void Agree(std::shared_ptr<NWebScreenCaptureConfig> config) = 0;

    /**
     * Refuse the request.
     */
    virtual void Refuse() = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_ACCESS_REQUEST_H
