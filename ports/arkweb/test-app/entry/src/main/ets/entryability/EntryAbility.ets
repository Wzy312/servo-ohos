import { AbilityConstant, ConfigurationConstant, UIAbility, Want } from '@kit.AbilityKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { window } from '@kit.ArkUI';

const DOMAIN = 0x0000;
const DEFAULT_URL = 'about:blank';

export default class EntryAbility extends UIAbility {
  // Which page to load at startup, chosen by the `page` launch parameter (see `resolvePage`).
  // Defaults to the single-webview browser so existing (servoperf) launches are unaffected.
  private targetPage: string = 'pages/Index';

  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    try {
      this.context.getApplicationContext().setColorMode(ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET);
    } catch (err) {
      hilog.error(DOMAIN, 'testTag', 'Failed to set colorMode. Cause: %{public}s', JSON.stringify(err));
    }
    this.targetPage = this.resolvePage(want);
    this.applyStartUrl(want);
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onCreate');
  }

  // `--ps page tabs|split` selects a multi-webview test page; anything else is the browser.
  // Page switching after launch is done in-app (nav buttons) or by force-stop + relaunch.
  private resolvePage(want: Want): string {
    if (want.parameters !== undefined) {
      const params = want.parameters as Record<string, Object>;
      const page = params['page'];
      if (typeof page === 'string') {
        if (page === 'tabs') {
          return 'pages/TabsPage';
        }
        if (page === 'split') {
          return 'pages/SplitPage';
        }
        if (page === 'dialog') {
          return 'pages/DialogPage';
        }
        if (page === 'controller') {
          return 'pages/ControllerPage';
        }
      }
    }
    return 'pages/Index';
  }

  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    this.applyStartUrl(want);
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onNewWant');
  }

  private applyStartUrl(want: Want): void {
    let url: string = '';
    let proxyUri: string = '';
    let ignoreCertErrors: boolean = false;
    if (typeof want.uri === 'string' && want.uri.length > 0) {
      url = want.uri;
    }
    if (want.parameters !== undefined) {
      const params = want.parameters as Record<string, Object>;
      if (url.length === 0) {
        const candidate = params['--url'];
        if (typeof candidate === 'string' && candidate.length > 0) {
          url = candidate;
        }
      }
      // `--proxy-uri=http://host:port` — Index.ets calls
      // webview.ProxyController.applyProxyOverride before loadUrl. Used by
      // servoperf's wpr-replay path for the arkweb engine.
      const proxy = params['--proxy-uri'];
      if (typeof proxy === 'string' && proxy.length > 0) {
        proxyUri = proxy;
      }
      // `--ignore-certificate-errors` (presence-detected). When set,
      // `onSslErrorEventReceive` auto-confirms self-signed cert errors,
      // needed for WPR replay's per-host leaf certs.
      if (params['--ignore-certificate-errors'] !== undefined) {
        ignoreCertErrors = true;
      }
    }
    if (url.length === 0) {
      url = DEFAULT_URL;
    }
    AppStorage.setOrCreate<string>('startUrl', url);
    AppStorage.setOrCreate<string>('proxyUri', proxyUri);
    AppStorage.setOrCreate<boolean>('ignoreCertErrors', ignoreCertErrors);
    hilog.info(DOMAIN, 'testTag',
      'startUrl=%{public}s proxy=%{public}s ignoreCertErrors=%{public}s',
      url, proxyUri.length > 0 ? proxyUri : '<none>', `${ignoreCertErrors}`);
  }

  onDestroy(): void {
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onDestroy');
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onWindowStageCreate');

    windowStage.loadContent(this.targetPage, (err) => {
      if (err.code) {
        hilog.error(DOMAIN, 'testTag', 'Failed to load the content. Cause: %{public}s', JSON.stringify(err));
        return;
      }
      hilog.info(DOMAIN, 'testTag', 'Succeeded in loading %{public}s', this.targetPage);
    });
  }

  onWindowStageDestroy(): void {
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onWindowStageDestroy');
  }

  onForeground(): void {
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onForeground');
  }

  onBackground(): void {
    hilog.info(DOMAIN, 'testTag', '%{public}s', 'Ability onBackground');
  }
}
