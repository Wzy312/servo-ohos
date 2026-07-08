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

#ifndef NWEB_DOWNLOAD_CALLBACK_H
#define NWEB_DOWNLOAD_CALLBACK_H

#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebDownloadCallback {
public:
    NWebDownloadCallback() = default;

    virtual ~NWebDownloadCallback() = default;
    /**
     * @brief Notify the host application that a file should be downloaded
     *
     * @param url The full url to the content that should be downloaded.
     * @param userAgent The user agent to be used for the download.
     * @param contentDisposition Content-disposition http header, if present.
     * @param mimetype The mimetype of the content reported by the server.
     * @param contentLength The file size reported by the server.
     */
    virtual void OnDownloadStart(const std::string& url, const std::string& userAgent,
        const std::string& contentDisposition, const std::string& mimetype, long contentLength) = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_DOWNLOAD_CALLBACK_H