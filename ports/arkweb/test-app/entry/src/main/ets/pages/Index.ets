import { webview } from '@kit.ArkWeb';
import { BusinessError } from '@kit.BasicServicesKit';
import { hilog, hiTraceMeter } from '@kit.PerformanceAnalysisKit';

const DOMAIN = 0x0000;

// Emit an instant marker into hitrace so servoperf's critical-path
// analyser can pick it up the same way as Servo's tracing-hitrace spans.
function markInstant(name: string, taskId: number): void {
  hiTraceMeter.startTrace(name, taskId);
  hiTraceMeter.finishTrace(name, taskId);
}

@Entry
@Component
struct Index {
  @StorageLink('startUrl') @Watch('onStartUrlChanged') startUrl: string = 'about:blank';
  @StorageProp('proxyUri') proxyUri: string = '';
  @StorageProp('ignoreCertErrors') ignoreCertErrors: boolean = false;
  @State urlBar: string = 'about:blank';
  @State currentUrl: string = 'about:blank';
  @State progress: number = 100;
  private controllerReady: boolean = false;
  private proxyApplied: boolean = false;
  private controller: webview.WebviewController = new webview.WebviewController();

  aboutToAppear(): void {
    if (this.startUrl.length > 0) {
      this.urlBar = this.startUrl;
      this.currentUrl = this.startUrl;
    }
  }

  /// Apply the launch-supplied proxy URI via webview.ProxyController. Async:
  /// the per-app proxy isn't active until the callback fires, so the caller
  /// (controller-attached handler / navigate) must defer loadUrl until then.
  private applyProxyThen(after: () => void): void {
    if (this.proxyUri.length === 0 || this.proxyApplied) {
      after();
      return;
    }
    try {
      const config = new webview.ProxyConfig();
      config.insertProxyRule(this.proxyUri, webview.ProxySchemeFilter.MATCH_ALL_SCHEMES);
      webview.ProxyController.applyProxyOverride(config, () => {
        this.proxyApplied = true;
        hilog.info(DOMAIN, 'arkwebtest', 'proxy applied: %{public}s', this.proxyUri);
        after();
      });
    } catch (err) {
      const e = err as BusinessError;
      hilog.error(DOMAIN, 'arkwebtest',
        'applyProxyOverride failed: %{public}d %{public}s', e.code, e.message);
      after();
    }
  }

  onStartUrlChanged(): void {
    if (this.startUrl.length === 0 || this.startUrl === this.currentUrl) {
      return;
    }
    this.urlBar = this.startUrl;
    this.currentUrl = this.startUrl;
    if (this.controllerReady) {
      try {
        this.controller.loadUrl(this.startUrl);
      } catch (err) {
        const e = err as BusinessError;
        hilog.error(DOMAIN, 'arkwebtest', 'loadUrl failed: %{public}d %{public}s', e.code, e.message);
      }
    }
  }

  private navigate(url: string): void {
    if (url.length === 0) {
      return;
    }
    this.currentUrl = url;
    if (!this.controllerReady) {
      return;
    }
    try {
      this.controller.loadUrl(url);
    } catch (err) {
      const e = err as BusinessError;
      hilog.error(DOMAIN, 'arkwebtest', 'loadUrl failed: %{public}d %{public}s', e.code, e.message);
    }
  }

  build() {
    Column() {
      Row({ space: 4 }) {
        Button('<')
          .onClick(() => {
            if (!this.controllerReady) return;
            try {
              if (this.controller.accessBackward()) {
                this.controller.backward();
              }
            } catch (_) { /* ignore */ }
          })
          .width(40)
        Button('>')
          .onClick(() => {
            if (!this.controllerReady) return;
            try {
              if (this.controller.accessForward()) {
                this.controller.forward();
              }
            } catch (_) { /* ignore */ }
          })
          .width(40)
        TextInput({ text: this.urlBar })
          .layoutWeight(1)
          .onChange((value: string) => {
            this.urlBar = value;
          })
          .onSubmit(() => {
            this.navigate(this.urlBar);
          })
        Button('Go')
          .onClick(() => {
            this.navigate(this.urlBar);
          })
          .width(50)
        Button('R')
          .onClick(() => {
            if (!this.controllerReady) return;
            try {
              this.controller.refresh();
            } catch (_) { /* ignore */ }
          })
          .width(40)
      }
      .width('100%')
      .padding(8)

      if (this.progress < 100) {
        Progress({ value: this.progress, total: 100 })
          .width('100%')
          .height(2)
      }

      Web({ src: this.currentUrl, controller: this.controller })
        .javaScriptAccess(true)
        .domStorageAccess(true)
        .mixedMode(MixedMode.All)
        .onControllerAttached(() => {
          this.controllerReady = true;
          hilog.info(DOMAIN, 'arkwebtest', 'controllerAttached, navigating to %{public}s', this.currentUrl);
          if (this.currentUrl.length === 0) {
            return;
          }
          this.applyProxyThen(() => {
            try {
              this.controller.loadUrl(this.currentUrl);
            } catch (err) {
              const e = err as BusinessError;
              hilog.error(DOMAIN, 'arkwebtest', 'initial loadUrl failed: %{public}d %{public}s', e.code, e.message);
            }
          });
        })
        // WPR replay uses per-host self-signed leaf certs; auto-confirm
        // when servoperf passed --ignore-certificate-errors.
        .onSslErrorEventReceive((event) => {
          if (this.ignoreCertErrors) {
            event?.handler?.handleConfirm();
          } else {
            event?.handler?.handleCancel();
          }
        })
        .onProgressChange((event) => {
          if (event !== undefined) {
            this.progress = event.newProgress;
          }
        })
        .onPageBegin((event) => {
          if (event !== undefined) {
            this.urlBar = event.url;
            markInstant('PageBegin', 1);
          }
        })
        .onPageEnd((event) => {
          if (event !== undefined) {
            this.urlBar = event.url;
            this.progress = 100;
            // Matches the milestone name servoperf already uses for Servo on OHOS.
            markInstant('PageLoadEndedPrompt', 2);
            markInstant('PageEnd', 3);
          }
        })
        // Fires on navigations that don't trigger a fresh page load, e.g.
        // history back/forward and history.pushState, so the URL bar stays in
        // sync where onPageBegin/onPageEnd don't fire.
        .onRefreshAccessedHistory((event) => {
          if (event !== undefined) {
            this.urlBar = event.url;
          }
        })
        .onFirstContentfulPaint(event => {
          markInstant('FirstContentfulPaint', 4);
          if (event !== undefined) {
            hiTraceMeter.traceByValue('FirstContentfulPaintMs', event.firstContentfulPaintMs);
            hilog.info(DOMAIN, 'arkwebtest',
              'FCP: navigationStartTick=%{public}d firstContentfulPaintMs=%{public}d',
              event.navigationStartTick, event.firstContentfulPaintMs);
          }
        })
        .onFirstMeaningfulPaint((details) => {
          markInstant('FirstMeaningfulPaint', 5);
          // firstMeaningfulPaintTime is already a millisecond delta from
          // navigationStartTime (the doc says "time taken … in milliseconds").
          const fmp = details?.firstMeaningfulPaintTime;
          if (fmp !== undefined) {
            hiTraceMeter.traceByValue('FirstMeaningfulPaintMs', fmp);
            hilog.info(DOMAIN, 'arkwebtest',
              'FMP: firstMeaningfulPaintMs=%{public}d navigationStartTimeUs=%{public}d',
              fmp, details?.navigationStartTime ?? -1);
          }
        })
        .onLargestContentfulPaint((details) => {
          markInstant('LargestContentfulPaint', 6);
          // largestImagePaintTime / largestTextPaintTime are millisecond
          // deltas from navigation start — same units as the FCP delta.
          const imgPaint = details?.largestImagePaintTime;
          const txtPaint = details?.largestTextPaintTime;
          const lcpEnd = Math.max(imgPaint ?? 0, txtPaint ?? 0);
          if (lcpEnd > 0) {
            hiTraceMeter.traceByValue('LargestContentfulPaintMs', lcpEnd);
            hilog.info(DOMAIN, 'arkwebtest',
              'LCP: largestImagePaintMs=%{public}d largestTextPaintMs=%{public}d navigationStartTimeUs=%{public}d',
              imgPaint ?? -1, txtPaint ?? -1, details?.navigationStartTime ?? -1);
          }
        })
        .layoutWeight(1)
        .width('100%')
    }
    .width('100%')
    .height('100%')
  }
}
