/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
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

#ifndef NWEB_NATIVE_VIDEO_PLAYER_H
#define NWEB_NATIVE_VIDEO_PLAYER_H

#include <map>
#include <memory>
#include <string>

namespace OHOS::NWeb {

enum class Preload { NONE = 0, METADATA, AUTO };

enum class MediaType { VIDEO = 0, AUDIO };

enum class SourceType { URL = 0, MSE };

enum class MediaError { NETWORK_ERROR = 1, FORMAT_ERROR, DECODE_ERROR };

enum class ReadyState { HAVE_NOTHING = 0, HAVE_METADATA, HAVE_CURRENT_DATA, HAVE_FUTURE_DATA, HAVE_ENOUGH_DATA };

enum class SuspendType { ENTER_BACK_FORWARD_CACHE = 0, ENTER_BACKGROUND, AUTO_CLEANUP };

enum class NetworkState { EMPTY = 0, IDLE, LOADING, NETWORK_ERROR };

enum class PlaybackStatus { PAUSED = 0, PLAYING };

class NWebMediaSourceInfo {
public:
    virtual ~NWebMediaSourceInfo() = default;

    virtual SourceType GetType() = 0;

    virtual std::string GetFormat() = 0;

    virtual std::string GetSource() = 0;
};

class NWebNativeMediaPlayerSurfaceInfo {
public:
    virtual ~NWebNativeMediaPlayerSurfaceInfo() = default;

    virtual std::string GetId() = 0;

    virtual double GetX() = 0;

    virtual double GetY() = 0;

    virtual double GetWidth() = 0;

    virtual double GetHeight() = 0;
};

class NWebMediaInfo {
public:
    virtual ~NWebMediaInfo() = default;

    virtual Preload GetPreload() = 0;

    virtual bool GetIsMuted() = 0;

    virtual std::string GetEmbedId() = 0;

    virtual std::string GetPosterUrl() = 0;

    virtual MediaType GetMediaType() = 0;

    virtual bool GetIsControlsShown() = 0;

    virtual std::vector<std::string> GetControls() = 0;

    virtual std::map<std::string, std::string> GetHeaders() = 0;

    virtual std::map<std::string, std::string> GetAttributes() = 0;

    virtual std::vector<std::shared_ptr<NWebMediaSourceInfo>> GetSourceInfos() = 0;

    virtual std::shared_ptr<NWebNativeMediaPlayerSurfaceInfo> GetSurfaceInfo() = 0;
};

class NWebNativeMediaPlayerHandler {
public:
    virtual ~NWebNativeMediaPlayerHandler() = default;

    virtual void HandleStatusChanged(PlaybackStatus status) = 0;

    virtual void HandleVolumeChanged(double volume) = 0;

    virtual void HandleMutedChanged(bool isMuted) = 0;

    virtual void HandlePlaybackRateChanged(double playbackRate) = 0;

    virtual void HandleDurationChanged(double duration) = 0;

    virtual void HandleTimeUpdate(double playTime) = 0;

    virtual void HandleBufferedEndTimeChanged(double bufferedEndTime) = 0;

    virtual void HandleEnded() = 0;

    virtual void HandleNetworkStateChanged(NetworkState state) = 0;

    virtual void HandleReadyStateChanged(ReadyState state) = 0;

    virtual void HandleFullScreenChanged(bool isFullScreen) = 0;

    virtual void HandleSeeking() = 0;

    virtual void HandleSeekFinished() = 0;

    virtual void HandleError(MediaError error, const std::string& message) = 0;

    virtual void HandleVideoSizeChanged(double width, double height) = 0;
};

class NWebNativeMediaPlayerBridge {
public:
    virtual ~NWebNativeMediaPlayerBridge() = default;

    virtual void UpdateRect(double x, double y, double width, double height) = 0;

    virtual void Play() = 0;

    virtual void Pause() = 0;

    virtual void Seek(double time) = 0;

    virtual void SetVolume(double volume) = 0;

    virtual void SetMuted(bool isMuted) = 0;

    virtual void SetPlaybackRate(double playbackRate) = 0;

    virtual void Release() = 0;

    virtual void EnterFullScreen() = 0;

    virtual void ExitFullScreen() = 0;

    virtual void ResumeMediaPlayer() = 0;

    virtual void SuspendMediaPlayer(SuspendType type) = 0;
};

class NWebCreateNativeMediaPlayerCallback {
public:
    virtual ~NWebCreateNativeMediaPlayerCallback() = default;

    virtual std::shared_ptr<NWebNativeMediaPlayerBridge> OnCreate(
        std::shared_ptr<NWebNativeMediaPlayerHandler> handler, std::shared_ptr<NWebMediaInfo> mediaInfo) = 0;
};

} // namespace OHOS::NWeb

#endif // NWEB_NATIVE_VIDEO_PLAYER_H
