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

#ifndef NWEB_FIND_CALLBACK_H
#define NWEB_FIND_CALLBACK_H

#include "nweb_export.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebFindCallback {
public:
    NWebFindCallback() = default;

    virtual ~NWebFindCallback() = default;

    /**
     * @brief Notify the host application that onFindResultReceived
     *
     * @param activeMatchOrdinal int: the zero-based ordinal of the currently
     * selected match
     * @param numberOfMatches int: how many matches have been found
     * @param isDoneCounting bool: whether the find operation has actually
     * completed. The listener may be notified multiple times while the operation
     * is underway, and the numberOfMatches value should not be considered final
     * unless isDoneCounting is true.
     */
    virtual void OnFindResultReceived(
        const int activeMatchOrdinal, const int numberOfMatches, const bool isDoneCounting) = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_FIND_CALLBACK_H
