const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const repo = path.resolve(__dirname, '..');
const mocks = fs.readFileSync(path.join(repo, 'node_modules/@tauri-apps/api/mocks.js'), 'utf8').replace(/^export .*$/gm, '');
const channel = process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge';
const reportPath = path.join(repo, 'release/validation/updater-regression.json');

function initialization(options = {}) {
  return mocks + `
    mockWindows('details');
    const options = ${JSON.stringify(options)};
    const appState = {generation:1,currentAccountId:null,selectedRange:'today',accounts:[],snapshot:null,
      settings:{theme:options.theme||'light',opacity:1,alwaysOnTop:true,recentRefreshSeconds:10,summaryRefreshSeconds:30,backgroundRefreshSeconds:120}};
    let updateState={revision:0,currentVersion:'0.1.8',distribution:options.distribution||'installed',channel:'preview',phase:'idle',autoCheck:true,
      checkedAt:null,version:null,notes:null,publishedAt:null,downloadedBytes:0,totalBytes:null,error:null,skippedVersion:null};
    const probe=window.__updateProbe={calls:[],activeListeners:new Set(),listenCalls:0,unlistenCalls:0,
      holdStatus:!!options.holdStatus,holdSubscription:!!options.holdSubscription};
    const snapshot=()=>structuredClone(updateState);
    probe.publish=async patch=>{updateState={...updateState,...patch,revision:updateState.revision+1};
      await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:update',payload:snapshot()});return snapshot();};
    probe.state=()=>snapshot();
    mockIPC(async(command,args)=>{
      if(command==='bootstrap')return appState;
      if(!['update_status','check_update','download_update','install_update','save_update_preferences','open_update_release'].includes(command))return null;
      probe.calls.push({command,args:structuredClone(args||{})});
      if(command==='update_status'){
        const captured=snapshot();
        if(probe.holdStatus){probe.holdStatus=false;await new Promise(resolve=>{probe.releaseStatus=resolve;});}
        return captured;
      }
      if(command==='check_update'){
        await probe.publish({phase:'checking',version:null,notes:null,error:null,downloadedBytes:0,totalBytes:null});
        const reply={...snapshot(),revision:updateState.revision+1,phase:'available',checkedAt:'2026-10-03T02:00:00Z',version:'0.1.9',notes:'隔离测试更新说明 <img src=x onerror="window.__unsafe=true">',publishedAt:'2026-10-03T01:00:00Z'};
        if(probe.holdCheck){await new Promise(resolve=>{probe.releaseCheck=resolve;});}
        if(probe.failCheck){probe.failCheck=false;await probe.publish({phase:'error',error:'更新渠道尚未发布'});throw new Error('更新渠道尚未发布');}
        if(reply.revision>updateState.revision){updateState=reply;await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:update',payload:snapshot()});}
        return reply;
      }
      if(command==='download_update'){
        await probe.publish({phase:'downloading',downloadedBytes:1024,totalBytes:null,error:null});
        if(probe.holdDownload){await new Promise(resolve=>{probe.releaseDownload=resolve;});}
        if(probe.failDownload){probe.failDownload=false;await probe.publish({phase:'error',downloadedBytes:0,totalBytes:null,error:'下载失败，请重试'});throw new Error('下载失败，请重试');}
        return probe.publish({phase:'ready',downloadedBytes:8192,totalBytes:null,error:null});
      }
      if(command==='install_update'){
        if(probe.holdInstall){await new Promise(resolve=>{probe.releaseInstall=resolve;});}
        if(probe.failInstall){probe.failInstall=false;await probe.publish({phase:'ready',error:'安装器未能启动，请重试'});throw new Error('安装器未能启动，请重试');}
        await probe.publish({phase:'installing',error:null});return null;
      }
      if(command==='save_update_preferences'){
        if(probe.failPreferences){probe.failPreferences=false;throw new Error('更新偏好未能保存');}
        return probe.publish({...('autoCheck' in args?{autoCheck:args.autoCheck}:{}),...('skipVersion' in args?{skippedVersion:args.skipVersion}:{})});
      }
      return null;
    },{shouldMockEvents:true});
    const invoke=window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke=async(command,args,extra)=>{
      if(command==='plugin:event|listen'&&args.event==='subgauge:update'){
        probe.listenCalls++;
        if(probe.holdSubscription){probe.holdSubscription=false;await new Promise(resolve=>{probe.releaseSubscription=resolve;});}
        const id=await invoke(command,args,extra);probe.activeListeners.add(id);return id;
      }
      if(command==='plugin:event|unlisten'&&args.event==='subgauge:update'){
        probe.unlistenCalls++;probe.activeListeners.delete(args.eventId);
        // The current official mock indexes args.id; real Tauri uses eventId.
        return invoke(command,{...args,id:args.eventId},extra);
      }
      return invoke(command,args,extra);
    };
  `;
}

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ root: repo, server: { host: '127.0.0.1', port: 0, strictPort: false, open: false } });
  const checks = [];
  let browser;
  const check = (name, passed) => { checks.push({ name, passed: !!passed }); assert(passed, name); };
  try {
    await server.listen();
    browser = await chromium.launch({ channel, headless: true });
    const base = `http://127.0.0.1:${server.httpServer.address().port}`;
    async function createPage(options) {
      const page = await browser.newPage({ viewport: { width: 960, height: 900 } });
      await page.addInitScript(initialization(options));
      await page.goto(`${base}/?window=details&page=settings`);
      await page.getByTestId('update-panel').waitFor();
      return page;
    }
    const page = await createPage({ holdStatus: true });
    await page.waitForFunction(() => typeof window.__updateProbe.releaseStatus === 'function');
    await page.evaluate(async () => { await window.__updateProbe.publish({ phase: 'available', version: '0.1.9', notes: '初始化竞态的新数据' }); window.__updateProbe.releaseStatus(); });
    await page.getByRole('status').filter({ hasText: '发现新版 0.1.9' }).waitFor();
    await page.getByRole('button', { name: '检查更新', exact: true }).waitFor();
    check('initial-status-does-not-overwrite-newer-subscribed-event', await page.getByText('初始化竞态的新数据', { exact: true }).count() === 1);
    check('update-panel-current-version-distribution-channel', (await page.getByTestId('update-panel').innerText()).includes('0.1.8') && (await page.getByTestId('update-panel').innerText()).includes('安装版 · 预览渠道'));
    check('opening-settings-does-not-check-download-or-install', await page.evaluate(() => window.__updateProbe.calls.every(call => call.command === 'update_status')));

    await page.evaluate(() => { window.__updateProbe.holdCheck = true; });
    await page.getByRole('button', { name: '检查更新', exact: true }).click();
    await page.waitForFunction(() => typeof window.__updateProbe.releaseCheck === 'function');
    await page.getByRole('button', { name: '检查中…', exact: true }).evaluate(button => { button.click(); button.click(); });
    check('duplicate-check-clicks-start-one-manual-request', await page.evaluate(() => window.__updateProbe.calls.filter(call => call.command === 'check_update').length === 1 && window.__updateProbe.calls.find(call => call.command === 'check_update').args.manual === true));
    await page.evaluate(async () => { await window.__updateProbe.publish({ phase: 'checking' }); await window.__updateProbe.publish({ phase: 'ready', version: '0.2.0', notes: '较新外部事件', downloadedBytes: 10, totalBytes: 10 }); window.__updateProbe.holdCheck = false; window.__updateProbe.releaseCheck(); });
    await page.getByRole('button', { name: '安装并重启', exact: true }).waitFor();
    check('stale-check-response-cannot-overwrite-ready-event', await page.getByText('较新外部事件', { exact: true }).count() === 1 && await page.getByRole('button', { name: '下载更新', exact: true }).count() === 0);
    check('ready-event-does-not-auto-install', await page.evaluate(() => window.__updateProbe.calls.every(call => call.command !== 'install_update')));

    await page.evaluate(() => { window.__updateProbe.failCheck = true; });
    await page.getByRole('button', { name: '检查更新', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '更新渠道尚未发布' }).waitFor();
    check('unpublished-channel-failure-is-visible-and-retryable', await page.getByRole('button', { name: '检查更新', exact: true }).isEnabled() && await page.getByRole('button', { name: '下载更新', exact: true }).count() === 0);
    await page.getByRole('button', { name: '检查更新', exact: true }).click();
    await page.getByRole('button', { name: '下载更新', exact: true }).waitFor();
    check('release-notes-render-as-text', await page.locator('.update-notes img').count() === 0 && await page.evaluate(() => !window.__unsafe));

    await page.getByRole('button', { name: '稍后', exact: true }).click();
    check('postpone-retains-version-without-downloading', await page.getByRole('button', { name: '查看更新', exact: true }).count() === 1 && await page.getByRole('button', { name: '下载更新', exact: true }).count() === 0);
    await page.getByRole('button', { name: '查看更新', exact: true }).click();
    await page.getByRole('button', { name: '跳过此版本', exact: true }).click();
    await page.getByRole('button', { name: '恢复提醒', exact: true }).waitFor();
    check('skip-version-is-saved-without-touching-auto-check', await page.evaluate(() => { const call = window.__updateProbe.calls.filter(call => call.command === 'save_update_preferences').at(-1); return JSON.stringify(call.args) === '{"skipVersion":"0.1.9"}'; }));
    await page.getByRole('button', { name: '恢复提醒', exact: true }).click();
    await page.getByRole('button', { name: '跳过此版本', exact: true }).waitFor();
    check('restore-reminder-sends-explicit-null', await page.evaluate(() => { const call = window.__updateProbe.calls.filter(call => call.command === 'save_update_preferences').at(-1); return 'skipVersion' in call.args && call.args.skipVersion === null; }));
    await page.getByRole('checkbox', { name: '自动检查更新', exact: true }).uncheck();
    await page.waitForFunction(() => !document.querySelector('.update-auto-check input').checked);
    check('auto-check-preference-only-patches-auto-check', await page.evaluate(() => { const call = window.__updateProbe.calls.filter(call => call.command === 'save_update_preferences').at(-1); return JSON.stringify(call.args) === '{"autoCheck":false}'; }));
    await page.evaluate(() => { window.__updateProbe.failPreferences = true; });
    await page.getByRole('checkbox', { name: '自动检查更新', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '更新偏好未能保存' }).waitFor();
    check('failed-preference-write-restores-actual-checkbox', !await page.getByRole('checkbox', { name: '自动检查更新', exact: true }).isChecked());

    await page.evaluate(() => { window.__updateProbe.failDownload = true; });
    await page.getByRole('button', { name: '下载更新', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '下载失败，请重试' }).waitFor();
    check('download-failure-keeps-retry-without-install', await page.getByRole('button', { name: '重新下载', exact: true }).isEnabled() && await page.getByRole('button', { name: '安装并重启', exact: true }).count() === 0);
    await page.getByRole('button', { name: '打开发布页', exact: true }).click();
    check('installed-download-error-offers-verified-release-page-fallback', await page.evaluate(() => { const calls = window.__updateProbe.calls.filter(call => call.command === 'open_update_release'); return calls.length === 1 && calls[0].args.revision === window.__updateProbe.state().revision; }));
    await page.evaluate(() => { window.__updateProbe.holdDownload = true; });
    await page.getByRole('button', { name: '重新下载', exact: true }).click();
    await page.waitForFunction(() => typeof window.__updateProbe.releaseDownload === 'function');
    await page.getByText('已下载 1.0 KB · 总大小未知', { exact: true }).waitFor();
    check('download-without-content-length-shows-indeterminate-progress', await page.getByRole('progressbar', { name: '更新下载进度', exact: true }).getAttribute('value') === null && !(await page.getByTestId('update-panel').innerText()).includes('NaN'));
    check('download-is-bound-to-current-revision', await page.evaluate(() => { const calls = window.__updateProbe.calls.filter(call => call.command === 'download_update'); return calls.every(call => Number.isInteger(call.args.revision)) && calls.length === 2; }));
    await page.getByRole('button', { name: '账号管理', exact: true }).click();
    await page.getByRole('heading', { name: '账号管理', exact: true }).waitFor();
    await page.waitForFunction(() => window.__updateProbe.activeListeners.size === 0);
    check('download-does-not-block-account-page-or-leak-listener', await page.getByTestId('update-panel').count() === 0 && await page.evaluate(() => window.__updateProbe.unlistenCalls === 1));
    await page.evaluate(async () => { window.__updateProbe.holdDownload = false; window.__updateProbe.releaseDownload(); });
    await page.getByRole('button', { name: '应用设置', exact: true }).click();
    await page.getByRole('button', { name: '安装并重启', exact: true }).waitFor();
    check('return-to-settings-restores-ready-state-with-single-listener', await page.evaluate(() => window.__updateProbe.activeListeners.size === 1 && window.__updateProbe.listenCalls === 2 && window.__updateProbe.calls.every(call => call.command !== 'install_update')));
    await page.getByRole('button', { name: '安装并重启', exact: true }).click();
    await page.getByRole('button', { name: '确认安装并重启', exact: true }).waitFor();
    check('install-needs-explicit-second-confirmation', await page.evaluate(() => window.__updateProbe.calls.every(call => call.command !== 'install_update')));
    await page.getByRole('button', { name: '取消安装', exact: true }).click();
    check('cancel-install-leaves-download-ready', await page.getByRole('button', { name: '安装并重启', exact: true }).count() === 1);
    await page.evaluate(() => { window.__updateProbe.failInstall = true; window.__updateProbe.holdInstall = true; });
    await page.getByRole('button', { name: '安装并重启', exact: true }).click();
    await page.getByRole('button', { name: '确认安装并重启', exact: true }).evaluate(button => { button.click(); button.click(); button.click(); });
    await page.waitForFunction(() => typeof window.__updateProbe.releaseInstall === 'function');
    await page.evaluate(() => { window.__updateProbe.holdInstall = false; window.__updateProbe.releaseInstall(); });
    await page.getByRole('alert').filter({ hasText: '安装器未能启动，请重试' }).waitFor();
    await page.getByRole('button', { name: '重试安装并重启', exact: true }).waitFor();
    check('install-failure-preserves-ready-and-deduplicates-clicks', await page.evaluate(() => window.__updateProbe.calls.filter(call => call.command === 'install_update').length === 1 && window.__updateProbe.state().phase === 'ready'));
    check('installed-pre-launch-failure-keeps-manual-release-fallback', await page.getByRole('button', { name: '打开发布页', exact: true }).isEnabled());
    await page.getByRole('button', { name: '重试安装并重启', exact: true }).click();
    await page.getByRole('button', { name: '确认安装并重启', exact: true }).click();
    await page.getByRole('status').filter({ hasText: '正在安装，即将重启…' }).waitFor();
    check('confirmed-install-retry-calls-native-with-revision', await page.evaluate(() => { const calls = window.__updateProbe.calls.filter(call => call.command === 'install_update'); return calls.length === 2 && Number.isInteger(calls[1].args.revision); }));
    await page.close();

    for (const distribution of ['portable', 'development']) {
      const variant = await createPage({ distribution, theme: 'dark' });
      await variant.getByRole('button', { name: '检查更新', exact: true }).click();
      await variant.getByRole('status').filter({ hasText: '发现新版 0.1.9' }).waitFor();
      check(`${distribution}-never-offers-native-install-or-download`, await variant.getByRole('button', { name: '下载更新', exact: true }).count() === 0 && await variant.getByRole('button', { name: '安装并重启', exact: true }).count() === 0);
      check(`${distribution}-inherits-dark-theme`, await variant.locator('html').getAttribute('data-theme') === 'dark');
      if (distribution === 'portable') {
        await variant.getByRole('button', { name: '下载 ZIP', exact: true }).click();
        check('portable-opens-only-native-verified-release-page', await variant.evaluate(() => { const calls = window.__updateProbe.calls.filter(call => call.command === 'open_update_release'); return calls.length === 1 && Number.isInteger(calls[0].args.revision) && window.__updateProbe.calls.every(call => !['download_update', 'install_update'].includes(call.command)); }));
      }
      await variant.evaluate(() => window.__updateProbe.publish({ phase: 'ready', downloadedBytes: 8192, totalBytes: 8192 }));
      await variant.getByRole('status').filter({ hasText: '下载与签名验证已完成' }).waitFor();
      check(`${distribution}-rejects-install-even-with-unexpected-ready-event`, await variant.getByRole('button', { name: '安装并重启', exact: true }).count() === 0 && await variant.getByRole('button', { name: '确认安装并重启', exact: true }).count() === 0);
      await variant.close();
    }
    const late = await createPage({ holdSubscription: true });
    await late.waitForFunction(() => typeof window.__updateProbe.releaseSubscription === 'function');
    await late.getByRole('button', { name: '账号管理', exact: true }).click();
    await late.getByRole('heading', { name: '账号管理', exact: true }).waitFor();
    await late.evaluate(() => { window.__updateProbe.releaseSubscription(); });
    await late.waitForFunction(() => window.__updateProbe.unlistenCalls === 1);
    check('subscription-that-resolves-after-unmount-is-disposed', await late.evaluate(() => window.__updateProbe.activeListeners.size === 0 && window.__updateProbe.calls.every(call => call.command !== 'update_status')));
    await late.close();
    const demo = await browser.newPage({ viewport: { width: 960, height: 900 } });
    await demo.goto(`${base}/?demo=1&window=details&page=settings`);
    await demo.getByText('示例预览，不检查、下载或安装真实软件。', { exact: true }).waitFor();
    await demo.getByRole('button', { name: '检查更新', exact: true }).click();
    await demo.getByRole('status').filter({ hasText: '示例状态：当前版本已是最新' }).waitFor();
    check('demo-explicitly-states-synthetic-version-and-never-downloads', (await demo.getByTestId('update-panel').innerText()).includes('0.1.8（示例）') && await demo.getByRole('button', { name: '下载更新', exact: true }).count() === 0 && await demo.getByRole('checkbox', { name: '自动检查更新', exact: true }).isDisabled());
    await demo.close();

    const report = { scope: `Real Vue update panel, official Tauri IPC mocks, synthetic data, isolated headless browser (${channel}); no network updater or installer runs`, passed: checks.every(item => item.passed), checks };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { try { await browser?.close(); } finally { await server.close(); } }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
