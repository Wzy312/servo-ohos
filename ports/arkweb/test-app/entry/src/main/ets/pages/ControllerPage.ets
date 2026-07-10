import { webview } from '@kit.ArkWeb';
import { router } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { BusinessError } from '@kit.BasicServicesKit';

const DOMAIN = 0x0000;

// A page for exercising WebviewController APIs from the on-device pytest suite
// (tools/test_controller.py). Each button invokes one controller API and reports the outcome
// to hilog as `ArkWebTest: <name>=<value>`; the tests tap buttons via `uitest dumpLayout` and
// assert on the log markers.
function report(name: string, value: string): void {
  hilog.info(DOMAIN, 'ArkWebTest', '%{public}s=%{public}s', name, value);
}

// Tall page so pageUp/pageDown have room to scroll; the title is asserted by getTitle.
function tallPage(): string {
  const html = `<html><head><title>ctrl-page</title>
<meta name=viewport content="width=device-width,initial-scale=1"></head>
<body style="margin:0;height:6000px;background:linear-gradient(#eeeeee,#333333)"></body></html>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

// Distinctly named and coloured pages for the backOrForward history tests.
function colorPage(name: string, color: string): string {
  const html = `<html><head><title>${name}</title></head>
<body style="margin:0;background:${color}"></body></html>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

// Requests the current position on load and paints the outcome: green = got a position,
// red = permission denied (code 1), blue = granted but unavailable/timeout. Grey = still waiting.
function geoPage(): string {
  const html = `<html><head><title>geo-page</title></head>
<body style="margin:0;background:#dddddd">
<script>
navigator.geolocation.getCurrentPosition(
  function(p){document.body.style.background='#00cc00';},
  function(e){document.body.style.background=(e.code===1)?'#cc0000':'#2244cc';});
</script></body></html>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

@Entry
@Component
struct ControllerPage {
  @StorageProp('startUrl') startUrl: string = '';
  private controller: webview.WebviewController = new webview.WebviewController();

  private pageUrl(): string {
    return this.startUrl.length > 0 && this.startUrl !== 'about:blank' ? this.startUrl : tallPage();
  }

  private invoke(name: string, action: () => string): void {
    try {
      report(name, action());
    } catch (e) {
      report(name, `error:${(e as BusinessError).code}`);
    }
  }

  @Builder
  navBar() {
    Row({ space: 6 }) {
      Button('Browser').height(32).onClick(() => router.replaceUrl({ url: 'pages/Index' }))
      Button('Tabs').height(32).onClick(() => router.replaceUrl({ url: 'pages/TabsPage' }))
      Button('Split').height(32).onClick(() => router.replaceUrl({ url: 'pages/SplitPage' }))
      Button('Controller').height(32).enabled(false)
    }
    .width('100%')
    .padding(6)
  }

  @Builder
  apiButton(label: string, onClick: () => void) {
    Button(label)
      .height(30)
      .fontSize(11)
      .layoutWeight(1)
      .type(ButtonType.Normal)
      .onClick(onClick)
  }

  @Builder
  buttonGrid() {
    Column({ space: 2 }) {
      Row({ space: 2 }) {
        this.apiButton('getTitle', () => this.invoke('getTitle', () => this.controller.getTitle()))
        this.apiButton('getProgress', () => this.invoke('getProgress', () => `${this.controller.getProgress()}`))
        this.apiButton('getUrl', () => this.invoke('getUrl', () => this.controller.getUrl()))
        this.apiButton('getOrigUrl', () => this.invoke('getOrigUrl', () => this.controller.getOriginalUrl()))
      }
      Row({ space: 2 }) {
        this.apiButton('scrollY', () => {
          try {
            this.controller.runJavaScript('window.scrollY')
              .then((result: string) => report('scrollY', result))
              .catch((e: Object) => report('scrollY', 'rejected:' + JSON.stringify(e)));
          } catch (e) {
            report('scrollY', `error:${(e as BusinessError).code}`);
          }
        })
        this.apiButton('pageDn', () => this.invoke('pageDn', () => {
          this.controller.pageDown(false);
          return 'ok';
        }))
        this.apiButton('pageDnEnd', () => this.invoke('pageDnEnd', () => {
          this.controller.pageDown(true);
          return 'ok';
        }))
        this.apiButton('pageUp', () => this.invoke('pageUp', () => {
          this.controller.pageUp(false);
          return 'ok';
        }))
        this.apiButton('pageUpTop', () => this.invoke('pageUpTop', () => {
          this.controller.pageUp(true);
          return 'ok';
        }))
      }
      Row({ space: 2 }) {
        this.apiButton('loadA', () => this.invoke('loadA', () => {
          this.controller.loadUrl(colorPage('page-a', '#cc0000'));
          return 'ok';
        }))
        this.apiButton('loadB', () => this.invoke('loadB', () => {
          this.controller.loadUrl(colorPage('page-b', '#00cc00'));
          return 'ok';
        }))
        this.apiButton('loadC', () => this.invoke('loadC', () => {
          this.controller.loadUrl(colorPage('page-c', '#2244cc'));
          return 'ok';
        }))
        this.apiButton('back2', () => this.invoke('back2', () => {
          this.controller.backOrForward(-2);
          return 'ok';
        }))
        this.apiButton('fwd1', () => this.invoke('fwd1', () => {
          this.controller.backOrForward(1);
          return 'ok';
        }))
      }
      Row({ space: 2 }) {
        this.apiButton('precompile', () => {
          try {
            this.controller.precompileJavaScript('https://test.invalid/t.js', 'console.log(1)',
              { responseHeaders: [] })
              .then((code: number) => report('precompile', `resolved:${code}`))
              .catch((e: Object) => report('precompile', 'rejected:' + JSON.stringify(e)));
          } catch (e) {
            report('precompile', `error:${(e as BusinessError).code}`);
          }
        })
        this.apiButton('mediaState', () => this.invoke('mediaState', () => `${this.controller.getMediaPlaybackState()}`))
        this.apiButton('pauseMedia', () => this.invoke('pauseMedia', () => {
          this.controller.pauseAllMedia();
          return 'ok';
        }))
        this.apiButton('resumeMedia', () => this.invoke('resumeMedia', () => {
          this.controller.resumeAllMedia();
          return 'ok';
        }))
        this.apiButton('loadGeo', () => this.invoke('loadGeo', () => {
          this.controller.loadUrl(geoPage());
          return 'ok';
        }))
      }
    }
    .width('100%')
    .padding({ left: 6, right: 6, bottom: 4 })
  }

  build() {
    Column() {
      this.navBar()
      this.buttonGrid()
      Web({ src: this.pageUrl(), controller: this.controller })
        .javaScriptAccess(true)
        .layoutWeight(1)
        .width('100%')
        .onPageEnd((event) => {
          if (event) {
            report('pageEnd', event.url.substring(0, 80));
          }
        })
        .onGeolocationShow((event) => {
          if (event) {
            report('geoShow', event.origin);
            event.geolocation.invoke(event.origin, true, false);
          }
        })
        .onPermissionRequest((event) => {
          if (event) {
            report('permission', event.request.getOrigin());
            // Deny camera/microphone: Servo's OHOS getUserMedia backend is unimplemented, so
            // granting would let the page proceed into an unsupported capture path.
            event.request.deny();
          }
        })
        .onHttpAuthRequest((event) => {
          if (event) {
            report('httpAuth', `${event.host}|${event.realm}`);
            event.handler.confirm('user', 'passwd');
          }
          return true;
        })
    }
    .width('100%')
    .height('100%')
  }
}
