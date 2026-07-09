import { webview } from '@kit.ArkWeb';
import { router } from '@kit.ArkUI';

// See TabsPage: self-contained, visually distinct pages so each surface is identifiable offline.
function dataPage(bg: string, label: string): string {
  const html = `<body style="margin:0;height:100vh;background:${bg};color:#fff;`
    + `display:flex;align-items:center;justify-content:center;font-family:sans-serif">`
    + `<h1 style="font-size:7vw">${label}</h1></body>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

// Parallel multi-webview test: two `Web` components stacked top/bottom, both live and painting at
// the same time. This is the real multi-surface case — two NWeb/rendering contexts concurrently.
@Entry
@Component
struct SplitPage {
  @StorageProp('startUrl') startUrl: string = '';
  private topController: webview.WebviewController = new webview.WebviewController();
  private bottomController: webview.WebviewController = new webview.WebviewController();

  // Top surface honours a launch-supplied URL (`-U`); the bottom is always a built-in data: page.
  private topUrl(): string {
    return this.startUrl.length > 0 && this.startUrl !== 'about:blank'
      ? this.startUrl
      : dataPage('#c0392b', 'Surface A (top)');
  }

  @Builder
  navBar() {
    Row({ space: 6 }) {
      Button('Browser').height(32).onClick(() => router.replaceUrl({ url: 'pages/Index' }))
      Button('Tabs').height(32).onClick(() => router.replaceUrl({ url: 'pages/TabsPage' }))
      Button('Split').height(32).enabled(false)
    }
    .width('100%')
    .padding(6)
  }

  build() {
    Column() {
      this.navBar()
      Web({ src: this.topUrl(), controller: this.topController })
        .javaScriptAccess(true)
        .layoutWeight(1)
        .width('100%')
      Divider()
        .strokeWidth(2)
        .color(Color.White)
      Web({ src: dataPage('#2471a3', 'Surface B (bottom)'), controller: this.bottomController })
        .javaScriptAccess(true)
        .layoutWeight(1)
        .width('100%')
    }
    .width('100%')
    .height('100%')
  }
}
