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

#ifndef NWEB_ROM_VALUE_H
#define NWEB_ROM_VALUE_H

#include <map>
#include <memory>
#include <string>
#include <vector>

namespace OHOS::NWeb {

class NWebRomValue {
public:
    enum class Type : unsigned char {
        NONE = 0,
        BOOLEAN,
        INTEGER,
        DOUBLE,
        STRING,
        BINARY,
        DICTIONARY,
        LIST,
        ERROR,
        STRINGARRAY,
        BOOLEANARRAY,
        DOUBLEARRAY,
        INT64ARRAY
    };

    virtual ~NWebRomValue() = default;

    virtual Type GetType() = 0;

    virtual void SetType(Type type) = 0;

    virtual int GetInt() = 0;

    virtual void SetInt(int value) = 0;

    virtual bool GetBool() = 0;

    virtual void SetBool(bool value) = 0;

    virtual double GetDouble() = 0;

    virtual void SetDouble(double value) = 0;

    virtual std::string GetString() = 0;

    virtual void SetString(const std::string& value) = 0;

    virtual const char* GetBinary(int& length) = 0;

    virtual void SetBinary(int size, const char* value) = 0;

    virtual std::map<std::string, std::shared_ptr<NWebRomValue>> GetDictValue() = 0;

    virtual std::vector<std::shared_ptr<NWebRomValue>> GetListValue() = 0;

    virtual std::shared_ptr<NWebRomValue> NewChildValue() = 0;

    virtual void SaveDictChildValue(const std::string& key) = 0;

    virtual void SaveListChildValue() = 0;

    virtual int64_t GetInt64() = 0;

    virtual void SetInt64(int64_t value) = 0;

    virtual std::vector<uint8_t> GetBinary() = 0;

    virtual void SetBinary(const std::vector<uint8_t>& value) = 0;

    virtual std::vector<bool> GetBoolArray() = 0;

    virtual void SetBoolArray(const std::vector<bool>& value) = 0;

    virtual std::vector<int64_t> GetInt64Array() = 0;

    virtual void SetInt64Array(const std::vector<int64_t>& value) = 0;

    virtual std::vector<double> GetDoubleArray() = 0;

    virtual void SetDoubleArray(const std::vector<double>& value) = 0;

    virtual std::vector<std::string> GetStringArray() = 0;

    virtual void SetStringArray(const std::vector<std::string>& value) = 0;

    virtual std::string GetErrMsg() = 0;

    virtual void SetErrMsg(const std::string& msg) = 0;

    virtual std::string GetErrName() = 0;

    virtual void SetErrName(const std::string& name) = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_ROM_VALUE_H