import { webview } from '@kit.ArkWeb';
import { router } from '@kit.ArkUI';

// A page for exercising JS dialogs end-to-end. The Web component's onAlert/onConfirm/onPrompt
// handlers show real ArkUI dialogs and return the user's choice back to the page, so the engine's
// dialog round-trip (Servo -> shim -> ACE -> NWebJSDialogResult -> shim -> Servo) can be tested
// with a real response, not just the unhandled auto-dismiss.
//
// The built-in page has three buttons (Alert/Confirm/Prompt) and encodes the outcome as the body
// background colour so it can be checked from a screenshot:
//   alert   -> purple (acknowledged)
//   confirm -> green if OK, red if Cancel
//   prompt  -> green if OK (returns the default "hi"), red if Cancel
function dialogPage(): string {
  const html = `<body id=b style="margin:0;background:#dddddd;font-family:sans-serif">
<div style="display:flex">
<button style="flex:1;height:90px;font-size:22px" onclick="a()">Alert</button>
<button style="flex:1;height:90px;font-size:22px" onclick="c()">Confirm</button>
<button style="flex:1;height:90px;font-size:22px" onclick="p()">Prompt</button>
</div>
<script>
function bg(x){document.getElementById('b').style.background=x}
function a(){alert('Alert message');bg('#9b59b6')}
function c(){bg(confirm('Confirm message')?'#00cc00':'#cc0000')}
function p(){var r=prompt('Prompt message','hi');bg(r==='hi'?'#00cc00':(r===null?'#cc0000':'#2471a3'))}
</script>
</body>`;
  return 'data:text/html,' + encodeURIComponent(html);
}

@Entry
@Component
struct DialogPage {
  @StorageProp('startUrl') startUrl: string = '';
  private controller: webview.WebviewController = new webview.WebviewController();

  private pageUrl(): string {
    return this.startUrl.length > 0 && this.startUrl !== 'about:blank' ? this.startUrl : dialogPage();
  }

  @Builder
  navBar() {
    Row({ space: 6 }) {
      Button('Browser').height(32).onClick(() => router.replaceUrl({ url: 'pages/Index' }))
      Button('Tabs').height(32).onClick(() => router.replaceUrl({ url: 'pages/TabsPage' }))
      Button('Split').height(32).onClick(() => router.replaceUrl({ url: 'pages/SplitPage' }))
      Button('Dialog').height(32).enabled(false)
    }
    .width('100%')
    .padding(6)
  }

  build() {
    Column() {
      this.navBar()
      Web({ src: this.pageUrl(), controller: this.controller })
        .javaScriptAccess(true)
        .layoutWeight(1)
        .width('100%')
        .onAlert((event) => {
          if (event) {
            AlertDialog.show({
              title: 'Alert',
              message: event.message,
              confirm: { value: 'OK', action: () => { event.result.handleConfirm(); } },
              autoCancel: false,
            });
          }
          return true;
        })
        .onConfirm((event) => {
          if (event) {
            AlertDialog.show({
              title: 'Confirm',
              message: event.message,
              primaryButton: { value: 'OK', action: () => { event.result.handleConfirm(); } },
              secondaryButton: { value: 'Cancel', action: () => { event.result.handleCancel(); } },
              autoCancel: false,
            });
          }
          return true;
        })
        .onPrompt((event) => {
          if (event) {
            AlertDialog.show({
              title: 'Prompt',
              message: `${event.message}  (default: ${event.value})`,
              primaryButton: { value: 'OK', action: () => { event.result.handlePromptConfirm(event.value); } },
              secondaryButton: { value: 'Cancel', action: () => { event.result.handleCancel(); } },
              autoCancel: false,
            });
          }
          return true;
        })
    }
    .width('100%')
    .height('100%')
  }
}
