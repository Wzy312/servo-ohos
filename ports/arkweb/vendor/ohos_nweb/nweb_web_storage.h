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

#ifndef NWEB_WEB_STORAGE_H
#define NWEB_WEB_STORAGE_H

#include <memory>
#include <string>
#include <vector>

#include "nweb_export.h"
#include "nweb_value_callback.h"

namespace OHOS::NWeb {

class NWebWebStorageOrigin;
class OHOS_NWEB_EXPORT NWebWebStorage {
public:
    NWebWebStorage() = default;

    virtual ~NWebWebStorage() = default;

    virtual void DeleteAllData(bool incognito_mode) = 0;
    virtual int DeleteOrigin(const std::string& origin) = 0;
    virtual void GetOrigins(std::shared_ptr<NWebWebStorageOriginVectorValueCallback> callback) = 0;
    virtual std::vector<std::shared_ptr<NWebWebStorageOrigin>> GetOrigins() = 0;
    virtual void GetOriginQuota(const std::string& origin, std::shared_ptr<NWebLongValueCallback> callback) = 0;
    virtual long GetOriginQuota(const std::string& origin) = 0;
    virtual void GetOriginUsage(const std::string& origin, std::shared_ptr<NWebLongValueCallback> callback) = 0;
    virtual long GetOriginUsage(const std::string& origin) = 0;
};

class OHOS_NWEB_EXPORT NWebWebStorageOrigin {
public:
    NWebWebStorageOrigin() = default;
    virtual ~NWebWebStorageOrigin() = default;

    virtual void SetOrigin(const std::string& origin) = 0;
    virtual void SetQuota(long quota) = 0;
    virtual void SetUsage(long usage) = 0;
    virtual std::string GetOrigin() = 0;
    virtual long GetQuota() = 0;
    virtual long GetUsage() = 0;
};

} // namespace OHOS::NWeb

#endif // NWebWebStorage
