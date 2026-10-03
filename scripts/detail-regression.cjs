const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const repo = path.resolve(__dirname, '..');
const mocks = fs.readFileSync(path.join(repo, 'node_modules/@tauri-apps/api/mocks.js'), 'utf8').replace(/^export .*$/gm, '');
const channel = process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge';
const reportPath = path.join(repo, 'release/validation/detail-regression.json');

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ root: repo, server: { host: '127.0.0.1', port: 0, strictPort: false, open: false } });
  let browser;
  const checks = [];
  try {
    await server.listen();
    browser = await chromium.launch({ channel, headless: true });
    const page = await browser.newPage({ viewport: { width: 960, height: 820 } });
    const initScript = mocks + `
      mockWindows('details');
      const totals = {cost:'1.25',requests:1,inputTokens:100,outputTokens:30,cacheReadTokens:50,cacheCreationTokens:0};
      const state = {
        generation:1,currentAccountId:'synthetic',selectedRange:'today',
        accounts:[{id:'synthetic',site:'https://example.com',email:'demo@example.com',role:'user',needsLogin:false,demo:true,preferences:{alias:'隔离示例',defaultRange:'today',metrics:['cost','requests','tokens','cache'],recentMinutes:5,timezone:'Asia/Shanghai'}}],
        settings:{theme:'light',opacity:1,alwaysOnTop:true,recentRefreshSeconds:10,summaryRefreshSeconds:30,backgroundRefreshSeconds:120},
        snapshot:{accountId:'synthetic',generation:1,range:'today',timezone:'Asia/Shanghai',start:'2026-10-02T00:00:00Z',end:'2026-10-02T01:00:00Z',totals,recent:totals,latest:null,balance:'10.00',sync:{state:'synced',complete:true,message:null,usageSyncedAt:'2026-10-02T01:00:00Z',balanceSyncedAt:'2026-10-02T01:00:00Z'}}
      };
      window.__detailProbe={overviewQueries:[],failOverview:true,slowStarted:false,slowFinished:false,saves:0,savedAlias:null,settingsCalls:[],layoutCalls:[],drags:0,bootstraps:0};
      window.__probeState=state;
      mockIPC(async (command,args) => {
        if(command==='bootstrap'){window.__detailProbe.bootstraps++;return state;}
        if(command==='query_records'){
          const q=args.query;
          let model=q.page===2?'old-filtered-page-sentinel':'records-first-sentinel';
          if(q.pageSize===5){
            window.__detailProbe.overviewQueries.push(q);
            if(window.__detailProbe.failOverview){
              await new Promise(resolve=>{window.__detailProbe.releaseOverviewFailure=resolve;});
              throw new Error('示例网络失败');
            }
            model='fresh-overview-sentinel';
          }else if(q.model==='slow-result'){
            window.__detailProbe.slowStarted=true;
            await new Promise(resolve=>{window.__detailProbe.releaseSlowResponse=resolve;});
            // The next frame follows this response's promise chain and Vue DOM flush.
            requestAnimationFrame(()=>{window.__detailProbe.slowFinished=true;});
            model='late-records-sentinel';
          }
          return {accountId:'synthetic',generation:1,range:'today',page:q.page,pageSize:q.pageSize,total:q.pageSize===5?1:40,complete:true,items:[{id:q.page,createdAt:'2026-10-02T00:10:00Z',model,apiKeyId:1,apiKeyName:'示例',totals,durationMs:1000}]};
        }
        if(command==='analysis')return {accountId:'synthetic',generation:1,range:'today',start:state.snapshot.start,end:state.snapshot.end,timezone:'Asia/Shanghai',syncedAt:state.snapshot.end,trend:[],models:[],keys:[],complete:true};
        if(command==='save_preferences'){
          state.accounts[0].preferences=args.preferences;
          state.generation++;
          window.__detailProbe.saves++;
          window.__detailProbe.savedAlias=args.preferences.alias;
          return state;
        }
        if(command==='patch_settings'){
          window.__detailProbe.settingsCalls.push(structuredClone(args));
          if(window.__detailProbe.holdPatch){await new Promise(resolve=>{window.__detailProbe.releasePatch=resolve;});}
          if(window.__detailProbe.failPatch){window.__detailProbe.failPatch=false;throw new Error('示例置顶保存失败');}
          for(const key of Object.keys(args.patch)){if(args.expected && args.expected[key]!==state.settings[key])throw new Error('应用设置已在其他入口修改，请重新确认后保存。');}
          state.settings={...state.settings,...args.patch};state.generation++;
          const committed=structuredClone(state);
          if(window.__detailProbe.holdResponse){await new Promise(resolve=>{window.__detailProbe.releaseResponse=resolve;});}
          return committed;
        }
        if(command==='set_float_layout'){
          window.__detailProbe.layoutCalls.push(structuredClone(args));
          if(window.__detailProbe.holdMenuClose && !args.menuOpen){await new Promise(resolve=>{window.__detailProbe.releaseMenuClose=resolve;});}
          if(window.__detailProbe.failMenuClose && !args.menuOpen){window.__detailProbe.failMenuClose=false;throw new Error('示例菜单恢复失败');}
          return {width:316,height:args.height,manualHeight:false,resizing:false};
        }
        if(command==='start_float_drag'){window.__detailProbe.drags++;return null;}
        return null;
      },{shouldMockEvents:true});
    `;
    await page.addInitScript(initScript);
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/?window=details&page=records`);
    await page.getByText('records-first-sentinel', { exact: true }).waitFor();
    await page.getByRole('textbox', { name: '模型', exact: true }).fill('synthetic-filter');
    await page.getByRole('textbox', { name: 'Key ID', exact: true }).fill('1');
    await page.getByRole('button', { name: '筛选', exact: true }).click();
    await page.getByRole('button', { name: '下一页', exact: true }).click();
    await page.getByText('old-filtered-page-sentinel', { exact: true }).waitFor();
    await page.getByRole('button', { name: '用量概览', exact: true }).click();
    await page.waitForFunction(() => typeof window.__detailProbe.releaseOverviewFailure === 'function');
    checks.push({ name: 'overview-does-not-show-old-filtered-page-while-loading', passed: await page.getByText('old-filtered-page-sentinel', { exact: true }).count() === 0 });
    const overviewQuery = await page.evaluate(() => window.__detailProbe.overviewQueries[0]);
    checks.push({ name: 'overview-queries-first-five-without-record-filters', passed: overviewQuery.page === 1 && overviewQuery.pageSize === 5 && overviewQuery.model === null && overviewQuery.apiKeyId === null });
    await page.evaluate(() => window.__detailProbe.releaseOverviewFailure());
    await page.getByRole('alert').filter({ hasText: '示例网络失败' }).waitFor();
    checks.push({ name: 'overview-does-not-show-old-filtered-page-after-failure', passed: await page.getByText('old-filtered-page-sentinel', { exact: true }).count() === 0 });

    await page.evaluate(() => { window.__detailProbe.failOverview = false; });
    await page.getByRole('button', { name: '请求记录', exact: true }).click();
    await page.getByRole('textbox', { name: '模型', exact: true }).fill('slow-result');
    await page.getByRole('button', { name: '筛选', exact: true }).click();
    await page.waitForFunction(() => window.__detailProbe.slowStarted && typeof window.__detailProbe.releaseSlowResponse === 'function');
    await page.getByRole('button', { name: '用量概览', exact: true }).click();
    await page.getByText('fresh-overview-sentinel', { exact: true }).waitFor();
    await page.evaluate(() => window.__detailProbe.releaseSlowResponse());
    await page.waitForFunction(() => window.__detailProbe.slowFinished);
    checks.push({ name: 'late-records-response-cannot-overwrite-overview', passed: await page.getByText('fresh-overview-sentinel', { exact: true }).count() === 1 && await page.getByText('late-records-sentinel', { exact: true }).count() === 0 });

    await page.getByRole('button', { name: '账号管理', exact: true }).click();
    await page.getByRole('button', { name: '显示设置', exact: true }).click();
    await page.getByRole('textbox', { name: '账号名称', exact: true }).fill('已保存的示例');
    await page.getByRole('button', { name: '保存设置', exact: true }).click();
    await page.getByRole('heading', { name: '账号管理', exact: true }).waitFor();
    checks.push({ name: 'preferences-saved-through-mock-ipc', passed: await page.evaluate(() => window.__detailProbe.saves === 1 && window.__detailProbe.savedAlias === '已保存的示例') });
    checks.push({ name: 'preferences-success-notice-survives-destination-navigation', passed: await page.getByText('账号显示设置已保存', { exact: true }).count() === 1 });
    await page.getByRole('button', { name: '用量概览', exact: true }).click();
    await page.getByRole('heading', { name: '用量概览', exact: true }).waitFor();
    checks.push({ name: 'normal-navigation-clears-old-success-notice', passed: await page.getByText('账号显示设置已保存', { exact: true }).count() === 0 });

    await page.getByRole('button', { name: '应用设置', exact: true }).click();
    await page.getByRole('combobox', { name: '外观主题', exact: true }).selectOption('dark');
    await page.evaluate(() => {
      window.__probeState.settings.alwaysOnTop=false;
      window.__probeState.settings.summaryRefreshSeconds=60;
      window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:structuredClone(window.__probeState)});
    });
    await page.waitForFunction(() => document.querySelector('input[type=checkbox]').checked === false);
    checks.push({ name: 'unedited-settings-follow-external-update-while-draft-is-kept', passed: await page.getByRole('combobox', {name:'外观主题',exact:true}).inputValue()==='dark' && await page.getByRole('spinbutton',{name:'余额和用量（秒）',exact:true}).inputValue()==='60' });
    await page.getByRole('button',{name:'保存设置',exact:true}).click();
    await page.getByText('应用设置已保存',{exact:true}).waitFor();
    checks.push({ name: 'settings-submit-only-dirty-fields-and-their-base', passed: await page.evaluate(() => {const call=window.__detailProbe.settingsCalls.at(-1);return JSON.stringify(call.patch)==='{"theme":"dark"}' && JSON.stringify(call.expected)==='{"theme":"light"}' && window.__probeState.settings.alwaysOnTop===false && window.__probeState.settings.summaryRefreshSeconds===60;}) });

    await page.getByRole('combobox',{name:'外观主题',exact:true}).selectOption('light');
    await page.evaluate(() => {
      window.__probeState.settings.theme='system';window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:structuredClone(window.__probeState)});
    });
    await page.getByRole('status').filter({hasText:'未保存的修改仍保留'}).waitFor();
    const countBeforeConfirm=await page.evaluate(() => window.__detailProbe.settingsCalls.length);
    await page.getByRole('button',{name:'保存设置',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'请核对后再次点击'}).waitFor();
    checks.push({name:'same-field-conflict-keeps-draft-and-requires-confirmation',passed:await page.getByRole('combobox',{name:'外观主题',exact:true}).inputValue()==='light' && await page.evaluate(count=>window.__detailProbe.settingsCalls.length===count,countBeforeConfirm)});
    await page.getByRole('button',{name:'确认并保存',exact:true}).click();
    await page.getByText('应用设置已保存',{exact:true}).waitFor();
    checks.push({name:'reconfirmed-save-uses-refreshed-field-base',passed:await page.evaluate(()=>{const call=window.__detailProbe.settingsCalls.at(-1);return call.patch.theme==='light' && call.expected.theme==='system';})});

    const floatPage=await browser.newPage({viewport:{width:316,height:820}});
    await floatPage.addInitScript(initScript);
    await floatPage.goto(`http://127.0.0.1:${server.httpServer.address().port}/`);
    const pin=floatPage.locator('.pin-toggle');
    await pin.waitFor();
    checks.push({name:'float-pin-is-accessible-and-does-not-crowd-header',passed:await pin.getAttribute('aria-label')==='取消置顶' && await pin.getAttribute('aria-pressed')==='true' && await floatPage.locator('.float-header .pin-toggle').count()===0 && await pin.evaluate(button=>button.getBoundingClientRect().width>=28 && button.getBoundingClientRect().height>=28)});
    checks.push({name:'float-account-gear-has-clear-tooltip',passed:await floatPage.getByRole('button',{name:'当前账号显示设置',exact:true}).getAttribute('title')==='当前账号显示设置'});
    await floatPage.evaluate(()=>{window.__detailProbe.holdPatch=true;});
    await pin.click();
    await floatPage.waitForFunction(()=>typeof window.__detailProbe.releasePatch==='function');
    checks.push({name:'pin-request-only-disables-pin',passed:await pin.isDisabled() && await floatPage.getByRole('button',{name:'展开',exact:true}).isEnabled() && await floatPage.getByRole('combobox',{name:'统计范围',exact:true}).isEnabled()});
    await floatPage.evaluate(()=>{
      window.__probeState.settings.theme='dark';window.__probeState.settings.summaryRefreshSeconds=90;window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:structuredClone(window.__probeState)});
      window.__detailProbe.holdPatch=false;window.__detailProbe.releasePatch();
    });
    await floatPage.waitForFunction(()=>document.querySelector('.pin-toggle').getAttribute('aria-pressed')==='false' && !document.querySelector('.pin-toggle').disabled);
    checks.push({name:'pin-patch-preserves-concurrent-theme-and-refresh-update',passed:await floatPage.evaluate(()=>{const call=window.__detailProbe.settingsCalls.at(-1);return JSON.stringify(call.patch)==='{"alwaysOnTop":false}' && JSON.stringify(call.expected)==='{"alwaysOnTop":true}' && window.__probeState.settings.theme==='dark' && window.__probeState.settings.summaryRefreshSeconds===90;})});
    checks.push({name:'dark-pin-state-applies-theme',passed:await floatPage.locator('html').getAttribute('data-theme')==='dark'});

    await floatPage.evaluate(()=>{window.__detailProbe.failPatch=true;});
    await pin.click();
    await floatPage.getByRole('status').filter({hasText:'示例置顶保存失败'}).waitFor();
    checks.push({name:'failed-pin-save-does-not-fake-success',passed:await pin.getAttribute('aria-pressed')==='false' && await pin.isEnabled()});
    await floatPage.evaluate(()=>{window.__detailProbe.holdResponse=true;});
    await pin.click();
    await floatPage.waitForFunction(()=>typeof window.__detailProbe.releaseResponse==='function');
    await floatPage.evaluate(()=>{
      window.__probeState.settings.alwaysOnTop=false;window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:structuredClone(window.__probeState)});
      window.__detailProbe.holdResponse=false;window.__detailProbe.releaseResponse();
    });
    await floatPage.waitForFunction(()=>!document.querySelector('.pin-toggle').disabled);
    checks.push({name:'old-pin-response-cannot-overwrite-new-state',passed:await pin.getAttribute('aria-pressed')==='false'});

    await floatPage.getByRole('button',{name:'切换账号',exact:true}).click();
    await floatPage.waitForFunction(()=>window.__detailProbe.layoutCalls.some(call=>call.menuOpen));
    await floatPage.evaluate(()=>{window.__detailProbe.holdMenuClose=true;});
    await floatPage.getByRole('button',{name:'拖动浮窗',exact:true}).dispatchEvent('pointerdown',{button:0});
    await floatPage.waitForFunction(()=>typeof window.__detailProbe.releaseMenuClose==='function');
    checks.push({name:'drag-waits-for-menu-layout-restoration',passed:await floatPage.locator('.account-menu').count()===0 && await floatPage.evaluate(()=>window.__detailProbe.drags===0)});
    await floatPage.evaluate(()=>{window.__detailProbe.holdMenuClose=false;window.__detailProbe.releaseMenuClose();});
    await floatPage.waitForFunction(()=>window.__detailProbe.drags===1);
    checks.push({name:'drag-uses-native-command-after-closed-layout',passed:await floatPage.evaluate(()=>window.__detailProbe.layoutCalls.at(-1).menuOpen===false && window.__detailProbe.drags===1)});

    await floatPage.getByRole('button',{name:'切换账号',exact:true}).click();
    await floatPage.waitForFunction(()=>window.__detailProbe.layoutCalls.at(-1).menuOpen===true);
    await floatPage.evaluate(()=>{window.__detailProbe.failMenuClose=true;});
    await floatPage.getByRole('button',{name:'拖动浮窗',exact:true}).dispatchEvent('pointerdown',{button:0});
    await floatPage.getByRole('status').filter({hasText:'浮窗尺寸未能调整'}).waitFor();
    checks.push({name:'failed-menu-restoration-does-not-start-native-drag',passed:await floatPage.evaluate(()=>window.__detailProbe.drags===1)});

    await floatPage.getByRole('button',{name:'切换账号',exact:true}).click();
    await floatPage.waitForFunction(()=>window.__detailProbe.layoutCalls.at(-1).menuOpen===true);
    await floatPage.evaluate(()=>{window.__detailProbe.holdMenuClose=true;delete window.__detailProbe.releaseMenuClose;});
    await floatPage.getByRole('button',{name:'拖动浮窗',exact:true}).dispatchEvent('pointerdown',{button:0,pointerId:19});
    await floatPage.waitForFunction(()=>typeof window.__detailProbe.releaseMenuClose==='function');
    await floatPage.evaluate(()=>{
      window.dispatchEvent(new PointerEvent('pointerup',{pointerId:19}));
      window.__detailProbe.holdMenuClose=false;window.__detailProbe.releaseMenuClose();
    });
    await floatPage.waitForFunction(()=>!document.querySelector('.drag-handle').disabled);
    checks.push({name:'pointer-released-during-menu-restoration-does-not-start-late-drag',passed:await floatPage.evaluate(()=>window.__detailProbe.drags===1)});

    await floatPage.evaluate(()=>{
      window.__probeState.accounts=[];window.__probeState.currentAccountId=null;window.__probeState.snapshot=null;window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:structuredClone(window.__probeState)});
    });
    await floatPage.getByRole('button',{name:'连接账号',exact:true}).waitFor();
    checks.push({name:'welcome-card-keeps-pin-control',passed:await floatPage.locator('.welcome-footer .pin-toggle').count()===1});
    await pin.focus();await pin.press('Space');
    await floatPage.waitForFunction(()=>document.querySelector('.pin-toggle').getAttribute('aria-pressed')==='true');
    checks.push({name:'welcome-pin-supports-keyboard-toggle',passed:await pin.getAttribute('aria-label')==='取消置顶'});
    const bootstrapsBeforeError=await floatPage.evaluate(()=>window.__detailProbe.bootstraps);
    await floatPage.evaluate(()=>{
      window.__probeState.settings.alwaysOnTop=false;window.__probeState.generation++;
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:settings-error',payload:'示例托盘置顶未能保存'});
    });
    await floatPage.getByRole('alert').filter({hasText:'示例托盘置顶未能保存'}).waitFor();
    await floatPage.waitForFunction(()=>document.querySelector('.pin-toggle').getAttribute('aria-pressed')==='false');
    checks.push({name:'tray-settings-error-is-visible-and-refreshes-actual-pin-state',passed:await floatPage.evaluate(count=>window.__detailProbe.bootstraps===count+1,bootstrapsBeforeError)});
    await floatPage.close();
    const report = { scope: `Real Vue app, official Tauri mocks, synthetic data, isolated headless browser (${channel})`, passed: checks.every(check => check.passed), checks };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
    assert(report.passed, 'Detail regression checks failed');
  } finally { try { await browser?.close(); } finally { await server.close(); } }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
