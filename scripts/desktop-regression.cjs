const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const repo = path.resolve(__dirname, '..');
const mocks = fs.readFileSync(path.join(repo, 'node_modules/@tauri-apps/api/mocks.js'), 'utf8').replace(/^export .*$/gm, '');
const channel = process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge';
const reportPath = path.join(repo, 'release/validation/desktop-regression.json');

function initialization(options = {}) {
  return mocks + `
    mockWindows('details');
    const options = ${JSON.stringify(options)};
    let state = {revision:0,distribution:options.distribution||'installed',launchAtLogin:!!options.launchAtLogin,
      startupVisibility:'float',shortcut:null,shortcutRegistered:false,startupBlocked:options.startupBlocked===undefined?false:options.startupBlocked,error:null};
    const probe=window.__desktopProbe={calls:[],activeListeners:new Set(),listenCalls:0,unlistenCalls:0,
      holdStatus:!!options.holdStatus,holdSubscription:!!options.holdSubscription};
    const snapshot=()=>structuredClone(state);
    probe.state=snapshot;
    probe.emit=async payload=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:desktop-state',payload});
    probe.publish=async patch=>{state={...state,...patch,revision:state.revision+1};await probe.emit(snapshot());return snapshot();};
    mockIPC(async(command,args)=>{
      if(!['desktop_status','save_desktop_preferences','open_startup_settings'].includes(command))return null;
      probe.calls.push({command,args:structuredClone(args||{})});
      if(command==='open_startup_settings')return null;
      if(command==='desktop_status'){
        if(probe.failStatus){probe.failStatus=false;throw new Error('无法读取启动设置');}
        const captured=snapshot();
        if(probe.holdStatus){probe.holdStatus=false;await new Promise(resolve=>{probe.releaseStatus=resolve;});}
        return captured;
      }
      if(probe.holdSave){probe.holdSave=false;await new Promise(resolve=>{probe.releaseSave=resolve;});}
      if(probe.failSave){probe.failSave=false;throw new Error('快捷键已被其他软件占用，原设置已保留');}
      const input=args.input;
      if(input.expectedRevision!==state.revision)throw new Error('启动设置已在其他窗口修改，请重新载入后保存。');
      if(state.distribution!=='installed'&&input.patch.launchAtLogin!==undefined)throw new Error('当前版本不登记开机自启');
      if(state.distribution==='development'&&input.patch.shortcut!==undefined)throw new Error('开发版不占用系统快捷键');
      const patch={...input.patch};
      if(patch.shortcut!==undefined){
        if(patch.shortcut!==null){
          const parts=patch.shortcut.split('+').map(value=>value.trim());
          const key=parts.pop().toUpperCase(),mods=parts.map(value=>value.toLowerCase());
          if(!mods.some(value=>['ctrl','alt'].includes(value))||mods.some(value=>!['ctrl','alt','shift'].includes(value))||new Set(mods).size!==mods.length||!/^(?:[A-Z0-9]|F(?:[1-9]|10|11))$/.test(key))throw new Error('快捷键格式无效，请使用 Ctrl 或 Alt 搭配字母、数字或 F1–F11');
          patch.shortcut=['ctrl','alt','shift'].filter(value=>mods.includes(value)).map(value=>value[0].toUpperCase()+value.slice(1)).concat(key).join('+');
        }
        patch.shortcutRegistered=patch.shortcut!==null;
      }
      const committed=await probe.publish({...patch,error:null});
      if(probe.holdResponse){probe.holdResponse=false;await new Promise(resolve=>{probe.releaseResponse=resolve;});}
      return committed;
    },{shouldMockEvents:true});
    const invoke=window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke=async(command,args,extra)=>{
      if(command==='plugin:event|listen'&&args.event==='subgauge:desktop-state'){
        probe.listenCalls++;
        if(probe.holdSubscription){probe.holdSubscription=false;await new Promise(resolve=>{probe.releaseSubscription=resolve;});}
        const id=await invoke(command,args,extra);probe.activeListeners.add(id);return id;
      }
      if(command==='plugin:event|unlisten'&&args.event==='subgauge:desktop-state'){
        probe.unlistenCalls++;probe.activeListeners.delete(args.eventId);
        return invoke(command,{...args,id:args.eventId},extra);
      }
      return invoke(command,args,extra);
    };
  `;
}

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ root: repo, server: { host: '127.0.0.1', port: 0, strictPort: false, open: false }, plugins: [{
    name: 'isolated-desktop-regression',
    configureServer(server) {
      server.middlewares.use('/__desktop-regression.html', async (request, response, next) => {
        const html = `<!doctype html><html lang="zh-CN"><meta name="viewport" content="width=device-width,initial-scale=1"><body><main style="max-width:490px;margin:auto;padding:16px"><div id="fixture"></div></main><script type="module">
          import { createApp } from 'vue';
          import DesktopPanel from '/src/components/DesktopPanel.vue';
          import '/src/style.css';
          document.documentElement.dataset.theme=new URLSearchParams(location.search).get('theme')||'light';
          const app=createApp(DesktopPanel);app.mount('#fixture');window.__unmountDesktop=()=>app.unmount();
        </script></body></html>`;
        try {
          response.setHeader('Content-Type', 'text/html; charset=utf-8');
          response.end(await server.transformIndexHtml('/__desktop-regression.html', html));
        } catch (error) { next(error); }
      });
    },
  }] });
  const checks = [];
  let browser;
  const check = (name, passed) => { checks.push({ name, passed: !!passed }); assert(passed, name); };
  try {
    await server.listen();
    browser = await chromium.launch({ channel, headless: true });
    const base = `http://127.0.0.1:${server.httpServer.address().port}/__desktop-regression.html`;
    async function createPage(options = {}) {
      const page = await browser.newPage({ viewport: { width: options.width || 960, height: 900 } });
      await page.addInitScript(initialization(options));
      await page.goto(`${base}?theme=${options.theme || 'light'}`);
      await page.getByTestId('desktop-panel').waitFor();
      return page;
    }
    const page = await createPage({ holdStatus: true });
    await page.waitForFunction(() => typeof window.__desktopProbe.releaseStatus === 'function');
    await page.evaluate(async () => { await window.__desktopProbe.publish({ startupVisibility: 'tray' }); window.__desktopProbe.releaseStatus(); });
    await page.getByRole('combobox', { name: '自启时显示', exact: true }).waitFor();
    await page.waitForFunction(() => document.querySelector('select').value === 'tray');
    check('initial-stale-status-does-not-overwrite-subscribed-state', await page.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'tray');
    check('opening-panel-does-not-enable-autostart-or-register-shortcut', await page.evaluate(() => window.__desktopProbe.calls.every(call => call.command === 'desktop_status')));
    check('autostart-and-shortcut-default-off', !await page.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).isChecked() && !await page.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).isChecked());
    check('unchanged-settings-cannot-be-saved', await page.getByRole('button', { name: '保存启动设置', exact: true }).isDisabled());

    await page.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).check();
    check('editing-autostart-does-not-register-until-explicit-save', await page.evaluate(() => window.__desktopProbe.calls.every(call => call.command === 'desktop_status')));
    await page.evaluate(() => { window.__desktopProbe.holdSave = true; });
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.waitForFunction(() => typeof window.__desktopProbe.releaseSave === 'function');
    await page.getByRole('button', { name: '保存中…', exact: true }).evaluate(button => { button.click(); button.click(); });
    check('duplicate-save-clicks-deduplicated-and-inputs-disabled', await page.evaluate(() => window.__desktopProbe.calls.filter(call => call.command === 'save_desktop_preferences').length === 1) && await page.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).isDisabled());
    await page.evaluate(() => window.__desktopProbe.releaseSave());
    await page.getByText('启动设置已保存。', { exact: true }).waitFor();
    check('save-patches-only-user-change-with-expected-revision', await page.evaluate(() => JSON.stringify(window.__desktopProbe.calls.find(call => call.command === 'save_desktop_preferences').args) === '{"input":{"expectedRevision":1,"patch":{"launchAtLogin":true}}}'));

    await page.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).check();
    check('shortcut-example-only-fills-draft', await page.getByRole('textbox', { name: '全局快捷键', exact: true }).inputValue() === 'Ctrl+Alt+G' && await page.getByText('快捷键已关闭', { exact: true }).count() === 1);
    check('editing-clears-previous-save-success-feedback', await page.getByText('启动设置已保存。', { exact: true }).count() === 0);
    await page.getByRole('textbox', { name: '全局快捷键', exact: true }).fill('');
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '请输入快捷键' }).waitFor();
    check('empty-enabled-shortcut-never-sends-native-save', await page.evaluate(() => window.__desktopProbe.calls.filter(call => call.command === 'save_desktop_preferences').length === 1));
    await page.getByRole('textbox', { name: '全局快捷键', exact: true }).fill('Win+G');
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '快捷键格式无效' }).waitFor();
    check('native-invalid-shortcut-message-is-visible-without-false-registration', await page.getByRole('textbox', { name: '全局快捷键', exact: true }).inputValue() === 'Win+G' && await page.getByText('快捷键已关闭', { exact: true }).count() === 1);
    await page.getByRole('textbox', { name: '全局快捷键', exact: true }).fill('Ctrl+Alt+G');
    await page.evaluate(() => { window.__desktopProbe.failSave = true; });
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '快捷键已被其他软件占用' }).waitFor();
    check('failed-registration-keeps-draft-and-reports-actual-shortcut-off', await page.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).isChecked() && await page.getByText('快捷键已关闭', { exact: true }).count() === 1 && await page.evaluate(() => window.__desktopProbe.state().shortcut === null));
    await page.getByRole('textbox', { name: '全局快捷键', exact: true }).fill('alt+ctrl+g');
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.getByText('当前快捷键：Ctrl+Alt+G', { exact: true }).waitFor();
    check('successful-save-adopts-native-normalized-shortcut', await page.getByRole('textbox', { name: '全局快捷键', exact: true }).inputValue() === 'Ctrl+Alt+G');
    await page.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).uncheck();
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.getByText('快捷键已关闭', { exact: true }).waitFor();
    check('disable-shortcut-sends-explicit-null', await page.evaluate(() => JSON.stringify(window.__desktopProbe.calls.filter(call => call.command === 'save_desktop_preferences').at(-1).args.input.patch) === '{"shortcut":null}'));

    await page.getByRole('combobox', { name: '自启时显示', exact: true }).selectOption('float');
    await page.evaluate(() => window.__desktopProbe.publish({ launchAtLogin: false }));
    await page.getByRole('alert').filter({ hasText: '启动设置已在其他窗口修改' }).waitFor();
    check('newer-event-keeps-unsaved-draft-and-prevents-overwriting', await page.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'float' && await page.getByRole('button', { name: '保存启动设置', exact: true }).isDisabled());
    await page.getByRole('button', { name: '载入最新启动设置', exact: true }).click();
    check('reload-conflict-adopts-latest-state', await page.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'tray' && !await page.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).isChecked());
    await page.getByRole('combobox', { name: '自启时显示', exact: true }).selectOption('float');
    await page.evaluate(() => { window.__desktopProbe.holdResponse = true; });
    await page.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await page.waitForFunction(() => typeof window.__desktopProbe.releaseResponse === 'function');
    await page.evaluate(async () => { await window.__desktopProbe.publish({ startupVisibility: 'tray' }); window.__desktopProbe.releaseResponse(); });
    await page.getByText('已载入另一窗口更新后的启动设置。', { exact: true }).waitFor();
    check('stale-save-response-cannot-overwrite-newer-event', await page.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'tray');
    await page.evaluate(() => window.__desktopProbe.emit({ ...window.__desktopProbe.state(), revision: 0, startupVisibility: 'float' }));
    check('old-state-event-is-ignored', await page.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'tray');

    await page.evaluate(() => window.__desktopProbe.publish({ launchAtLogin: true, startupBlocked: true }));
    await page.getByText(/Windows 已禁用 SubGauge 自启/).waitFor();
    check('windows-disabled-startup-is-not-shown-as-enabled', await page.getByText(/此处保存不会覆盖系统的禁用选择/).count() === 1);
    await page.evaluate(() => window.__desktopProbe.publish({ startupBlocked: null }));
    await page.getByRole('status').filter({ hasText: /Windows 中的禁用状态未确认/ }).waitFor();
    check('unknown-windows-startup-status-is-explicit', await page.getByText(/Windows 已禁用 SubGauge 自启/).count() === 0);
    await page.getByRole('button', { name: 'Windows 启动应用设置', exact: true }).click();
    check('windows-startup-settings-opens-only-fixed-native-command', await page.evaluate(() => { const calls = window.__desktopProbe.calls.filter(call => call.command === 'open_startup_settings'); return calls.length === 1 && JSON.stringify(calls[0].args) === '{}'; }));
    await page.getByRole('combobox', { name: '自启时显示', exact: true }).selectOption('float');
    await page.evaluate(() => window.__desktopProbe.emit(window.__desktopProbe.state()));
    check('same-revision-state-event-does-not-conflict-with-draft', await page.getByRole('button', { name: '保存启动设置', exact: true }).isEnabled() && await page.getByRole('alert').filter({ hasText: '启动设置已在其他窗口修改' }).count() === 0);
    await page.evaluate(() => window.__unmountDesktop());
    await page.waitForFunction(() => window.__desktopProbe.activeListeners.size === 0);
    check('unmount-releases-native-event-listener', await page.evaluate(() => window.__desktopProbe.unlistenCalls === 1));
    await page.close();

    for (const distribution of ['portable', 'development']) {
      const variant = await createPage({ distribution, theme: 'dark', width: 420 });
      await variant.getByRole('combobox', { name: '自启时显示', exact: true }).waitFor();
      check(`${distribution}-does-not-allow-autostart`, await variant.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).isDisabled());
      check(`${distribution}-does-not-open-windows-startup-settings`, await variant.getByRole('button', { name: 'Windows 启动应用设置', exact: true }).count() === 0);
      check(`${distribution}-shortcut-capability-is-explicit`, await variant.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).isEnabled() === (distribution === 'portable'));
      await variant.getByRole('combobox', { name: '自启时显示', exact: true }).selectOption('tray');
      await variant.getByRole('button', { name: '保存启动设置', exact: true }).click();
      await variant.getByText('启动设置已保存。', { exact: true }).waitFor();
      check(`${distribution}-visibility-save-never-touches-system-registration`, await variant.evaluate(() => JSON.stringify(window.__desktopProbe.calls.filter(call => call.command === 'save_desktop_preferences').at(-1).args.input.patch) === '{"startupVisibility":"tray"}'));
      check(`${distribution}-dark-theme-narrow-layout-has-no-horizontal-overflow`, await variant.evaluate(() => document.documentElement.dataset.theme === 'dark' && document.documentElement.scrollWidth <= innerWidth && getComputedStyle(document.querySelector('select')).color !== 'rgb(38, 50, 63)'));
      await variant.close();
    }
    const late = await createPage({ holdSubscription: true });
    await late.waitForFunction(() => typeof window.__desktopProbe.releaseSubscription === 'function');
    await late.evaluate(() => { window.__unmountDesktop(); window.__desktopProbe.releaseSubscription(); });
    await late.waitForFunction(() => window.__desktopProbe.unlistenCalls === 1);
    check('subscription-resolving-after-unmount-is-disposed-before-status-read', await late.evaluate(() => window.__desktopProbe.activeListeners.size === 0 && window.__desktopProbe.calls.length === 0));
    await late.close();

    const retry = await createPage({ holdSubscription: true });
    await retry.waitForFunction(() => typeof window.__desktopProbe.releaseSubscription === 'function');
    await retry.evaluate(() => { window.__desktopProbe.failStatus = true; window.__desktopProbe.releaseSubscription(); });
    await retry.getByRole('alert').filter({ hasText: '无法读取启动设置' }).waitFor();
    await retry.getByRole('button', { name: '重新读取启动设置', exact: true }).click();
    await retry.getByRole('combobox', { name: '自启时显示', exact: true }).waitFor();
    check('failed-initial-status-is-retryable-without-system-action', await retry.evaluate(() => window.__desktopProbe.calls.length === 2 && window.__desktopProbe.calls.every(call => call.command === 'desktop_status')));
    await retry.close();

    const demo = await browser.newPage();
    await demo.goto(`${base}?demo=1`);
    await demo.getByText('示例预览，不登记开机自启，不占用系统快捷键。', { exact: true }).waitFor();
    check('demo-never-offers-real-autostart-or-shortcut-registration', await demo.getByRole('checkbox', { name: '登录 Windows 后自动启动', exact: true }).isDisabled() && await demo.getByRole('checkbox', { name: '启用全局显示 / 隐藏快捷键', exact: true }).isDisabled());
    check('demo-does-not-open-windows-startup-settings', await demo.getByRole('button', { name: 'Windows 启动应用设置', exact: true }).count() === 0);
    await demo.getByRole('combobox', { name: '自启时显示', exact: true }).selectOption('tray');
    await demo.getByRole('button', { name: '保存启动设置', exact: true }).click();
    await demo.getByText('启动设置已保存。', { exact: true }).waitFor();
    check('demo-startup-visibility-is-explicit-synthetic-preview', await demo.getByRole('combobox', { name: '自启时显示', exact: true }).inputValue() === 'tray');
    await demo.close();

    const report = { scope: `Real Vue DesktopPanel, official Tauri IPC mocks, isolated headless ${channel}; synthetic preferences only, no accounts, registry edits, global hotkeys or installers`, passed: checks.every(item => item.passed), checks };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { try { await browser?.close(); } finally { await server.close(); } }
})().catch(error => { console.error(error.stack || error.message); process.exitCode = 1; });
