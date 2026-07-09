import { webview } from '@kit.ArkWeb';
import { router } from '@kit.ArkUI';

// Distinct self-contained pages so each Web surface is visually identifiable without a network
// (the test device has none). Colour + label make it obvious which webview rendered which tab.
function dataPage(bg: string, label: string): string {
  const html = `<body style="margin:0;height:100vh;background:${bg};color:#fff;`
    + `display:flex;align-items:center;justify-content:center;font-family:sans-serif">`
    + `<h1 style="font-size:7vw">${label}</h1></body>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

// Tab-based multi-webview test: three `Web` components (one per tab), each its own
// WebviewController, exercising create + show/hide of several NWeb surfaces in one window.
@Entry
@Component
struct TabsPage {
  @StorageProp('startUrl') startUrl: string = '';
  @State activeIndex: number = 0;
  private controllers: webview.WebviewController[] = [
    new webview.WebviewController(),
    new webview.WebviewController(),
    new webview.WebviewController(),
  ];

  // Tab 1 honours a launch-supplied URL (`-U`); the rest use built-in data: pages.
  private firstUrl(): string {
    return this.startUrl.length > 0 && this.startUrl !== 'about:blank'
      ? this.startUrl
      : dataPage('#c0392b', 'Tab 1');
  }

  @Builder
  navBar() {
    Row({ space: 6 }) {
      Button('Browser').height(32).onClick(() => router.replaceUrl({ url: 'pages/Index' }))
      Button('Tabs').height(32).enabled(false)
      Button('Split').height(32).onClick(() => router.replaceUrl({ url: 'pages/SplitPage' }))
    }
    .width('100%')
    .padding(6)
  }

  build() {
    Column() {
      this.navBar()
      Tabs({ index: this.activeIndex }) {
        TabContent() {
          Web({ src: this.firstUrl(), controller: this.controllers[0] })
            .javaScriptAccess(true)
            .width('100%')
            .height('100%')
        }.tabBar('Tab 1')
        TabContent() {
          Web({ src: dataPage('#27ae60', 'Tab 2'), controller: this.controllers[1] })
            .javaScriptAccess(true)
            .width('100%')
            .height('100%')
        }.tabBar('Tab 2')
        TabContent() {
          Web({ src: dataPage('#2471a3', 'Tab 3'), controller: this.controllers[2] })
            .javaScriptAccess(true)
            .width('100%')
            .height('100%')
        }.tabBar('Tab 3')
      }
      .onChange((index: number) => {
        this.activeIndex = index;
      })
      .layoutWeight(1)
    }
    .width('100%')
    .height('100%')
  }
}
