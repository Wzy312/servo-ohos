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

#ifndef NWEB_URL_RESOURCE_RESPONSE_H
#define NWEB_URL_RESOURCE_RESPONSE_H

#include <map>
#include <memory>
#include <string>

namespace OHOS::NWeb {

class NWebResourceReadyCallback {
public:
    virtual ~NWebResourceReadyCallback() {}
    virtual void Continue() = 0;
    virtual void Cancel() = 0;
};

enum class NWebResponseDataType : int32_t {
    NWEB_STRING_TYPE,
    NWEB_FILE_TYPE,
    NWEB_RESOURCE_URL_TYPE,
    NWEB_BUFFER_TYPE,
};

class NWebUrlResourceResponse {
public:
    virtual ~NWebUrlResourceResponse() = default;

    /**
     * @brief get input stream
     *
     * @retval inputstream string
     */
    virtual std::string ResponseData() = 0;

    /**
     * @brief set input stream
     *
     * @param input_stream set inputstream for example: fread(buf, 1, sizeof(buf),
     * file)
     */
    virtual void PutResponseData(const std::string& input_stream) = 0;

    /**
     * @brief Construct a resource response with the given parameters.
     *
     * @param encoding encoding { "utf-8" }
     */
    virtual void PutResponseEncoding(const std::string& encoding) = 0;

    /**
     * @brief get encoding
     *
     * @retval encoding the resource response's encoding
     */
    virtual std::string ResponseEncoding() = 0;

    /**
     * @brief Construct a resource response with the given parameters.
     *
     * @param mime_type mime_type{ "text/html" }
     */
    virtual void PutResponseMimeType(const std::string& mime_type) = 0;

    /**
     * @brief Get mimetype
     *
     * @retval mimetype The resource response's MIME type
     */
    virtual std::string ResponseMimeType() = 0;

    /**
     * @brief Set ResponseHeaders
     *
     * @param response_headers response header
     */
    virtual void PutResponseHeaders(const std::map<std::string, std::string>& response_headers) = 0;

    /**
     * @brief Get ResponseHeaders
     *
     * @retval response headers
     */
    virtual std::map<std::string, std::string> ResponseHeaders() = 0;

    /**
     * @brief Set StatusCode And ReasonPhrase
     *
     * @param status_code status code
     * @param reasonphrase reason phrase
     */
    virtual void PutResponseStateAndStatuscode(int status_code, const std::string& reason_phrase) = 0;

    /**
     * @brief Get status code
     *
     * @retval status code
     */
    virtual int ResponseStatusCode() = 0;

    /**
     * @brief Get ReasonPhrase
     *
     * @retval errorcode reason
     */
    virtual std::string ResponseStatus() = 0;

    virtual void PutResponseDataStatus(bool isDataReady) = 0;

    virtual bool ResponseDataStatus() = 0;

    virtual bool ResponseIsFileHandle() = 0;

    virtual void PutResponseFileHandle(int fd) = 0;

    virtual int ResponseFileHandle() = 0;

    virtual void PutResponseResourceUrl(const std::string& url) = 0;

    virtual std::string ResponseResourceUrl() = 0;

    virtual NWebResponseDataType ResponseDataType() = 0;

    virtual void PutResponseReadyCallback(std::shared_ptr<NWebResourceReadyCallback> readyCallback) = 0;

    virtual void PutResponseDataBuffer(char* buffer, size_t bufferSize) = 0;

    virtual char* GetResponseDataBuffer() = 0;

    virtual size_t GetResponseDataBufferSize() = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_URL_RESOURCE_RESPONSE_H
