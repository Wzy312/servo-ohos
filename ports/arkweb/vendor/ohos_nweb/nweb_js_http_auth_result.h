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

#ifndef NWEB_JS_HTTP_AUTH_RESULT_H
#define NWEB_JS_HTTP_AUTH_RESULT_H

#include <string>

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebJSHttpAuthResult {
public:
    virtual ~NWebJSHttpAuthResult() = default;
    /**
     * @brief Handle a confirmation response from the user.
     */
    virtual bool Confirm(const std::string& userName, const std::string& pwd) = 0;
    /**
     * @brief Handle the result if the user cancelled the dialog.
     */
    virtual void Cancel() = 0;
    /**
     * @brief Handle a IsHttpAuthInfoSaved response from the user.
     */
    virtual bool IsHttpAuthInfoSaved() = 0;
};

} // namespace OHOS::NWeb

#endif