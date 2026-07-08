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

#ifndef NWEB_JAVASCRIPT_RESULT_CALLBACK_H
#define NWEB_JAVASCRIPT_RESULT_CALLBACK_H

#include <memory>
#include <string>
#include <vector>

#include "nweb_export.h"
#include "nweb_hap_value.h"
#include "nweb_value.h"

namespace OHOS::NWeb {

class OHOS_NWEB_EXPORT NWebJavaScriptResultCallBack {
public:
    NWebJavaScriptResultCallBack() = default;

    virtual ~NWebJavaScriptResultCallBack() = default;

    virtual std::shared_ptr<NWebValue> GetJavaScriptResult(std::vector<std::shared_ptr<NWebValue>> args,
        const std::string& method, const std::string& object_name, int32_t routing_id, int32_t object_id) = 0;

    virtual std::shared_ptr<NWebValue> GetJavaScriptResultFlowbuf(std::vector<std::shared_ptr<NWebValue>> args,
        const std::string& method, const std::string& object_name, int fd, int32_t routing_id, int32_t object_id) = 0;

    /* HasJavaScriptObjectMethods
     *
     * @param object_id: means the JavaScript object id
     * @param object_id: means the method name
     */
    virtual bool HasJavaScriptObjectMethods(int32_t object_id, const std::string& method_name) = 0;

    /* GetJavaScriptObjectMethods
     *
     * @param object_id: means the JavaScript object id
     */
    virtual std::shared_ptr<NWebValue> GetJavaScriptObjectMethods(int32_t object_id) = 0;

    /* RemoveJavaScriptObjectHolder
     *
     * @param holder: means the JavaScript object is holded by
     * it(routing_id)
     * @param object_id: means the JavaScript object id
     */
    virtual void RemoveJavaScriptObjectHolder(int32_t holder, int32_t object_id) = 0;

    // Remove Transient JavaScript Object
    virtual void RemoveTransientJavaScriptObject() = 0;

    virtual void GetJavaScriptResultV2(const std::vector<std::shared_ptr<NWebHapValue>>& args,
        const std::string& method, const std::string& object_name, int32_t routing_id, int32_t object_id,
        std::shared_ptr<NWebHapValue> result) = 0;

    virtual void GetJavaScriptResultFlowbufV2(const std::vector<std::shared_ptr<NWebHapValue>>& args,
        const std::string& method, const std::string& object_name, int fd, int32_t routing_id, int32_t object_id,
        std::shared_ptr<NWebHapValue> result) = 0;

    virtual void GetJavaScriptObjectMethodsV2(int32_t object_id, std::shared_ptr<NWebHapValue> result) = 0;
};

} // namespace OHOS::NWeb
#endif