const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const mockSource = fs.readFileSync(path.join(__dirname, '../node_modules/@tauri-apps/api/mocks.js'), 'utf8').replace(/^export .*$/gm, '');
const reportDirectory = path.resolve(__dirname, '../release/validation');
fs.mkdirSync(reportDirectory, {recursive: true});
const reportPath = path.join(reportDirectory, 'float-layout-browser.json');
const checks = [];
(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host:'127.0.0.1',port:0,strictPort:false,open:false } });
  await server.listen();
  let browser;
  try {
    browser = await chromium.launch({ channel:process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge', headless:true });
    const url = `http://127.0.0.1:${server.httpServer.address().port}/`;
    for (const scale of [1, 1.25, 1.5, 2]) {
      const context = await browser.newContext({ viewport: { width: 316, height: 240 }, deviceScaleFactor: scale });
      const page = await context.newPage();
      let maximumHeight = 1400, requests = [], manualHeight = null, width = 316, layoutDelay = 0, inFlightLayouts = 0, peakInFlightLayouts = 0;
      await page.exposeFunction('__resizeProbe', async args => {
        inFlightLayouts++; peakInFlightLayouts=Math.max(peakInFlightLayouts,inFlightLayouts);
        requests.push(args);
        if(layoutDelay)await new Promise(resolve=>setTimeout(resolve,layoutDelay));
        const height = Math.min(Math.ceil(args.menuOpen ? Math.max(args.height, manualHeight || 0) : manualHeight == null ? args.height : Math.max(manualHeight, args.minHeight || 150)), maximumHeight);
        await page.setViewportSize({ width, height });
        inFlightLayouts--;
        return {width,height,manualHeight:manualHeight != null,resizing:false};
      });
      await page.exposeFunction('__resetProbe', async () => { width = 316; manualHeight = null; const args = requests.at(-1); const height = Math.min(args?.height || 240, maximumHeight); await page.setViewportSize({width,height}); return {width,height,manualHeight:false,resizing:false}; });
      await page.addInitScript(mockSource + `
        mockWindows('float');
        const totals = { cost: '26.80', requests: 184, inputTokens: 1500000, outputTokens: 450000, cacheReadTokens: 8400000, cacheCreationTokens: 630000 };
        const accounts = Array.from({length: 2}, (_, i) => ({id:'sample-'+i, site:'https://sample.example.com', email:'demo@example.com', role:i?'user':'admin', preferences:{alias:i?'上游套餐':'示例账号', defaultRange:'today', metrics:['cost','requests','tokens','cache'], recentMinutes:5, timezone:'Asia/Shanghai'}, needsLogin:false, demo:true}));
        const state = { accounts, currentAccountId:accounts[0].id, selectedRange:'today', generation:1, settings:{theme:'light',opacity:1,alwaysOnTop:true,recentRefreshSeconds:10,summaryRefreshSeconds:30,backgroundRefreshSeconds:120}, snapshot:{accountId:accounts[0].id,generation:1,range:'today',start:'2026-10-02T00:00:00Z',end:'2026-10-02T03:54:00Z',timezone:'Asia/Shanghai',totals,recent:{...totals,cost:'0.07',requests:4},balance:'1300.00',latest:{id:1,model:'gpt-6.1-sol',createdAt:'2026-10-02T03:54:22Z',apiKeyId:1,apiKeyName:'示例',totals:{...totals,cost:'0.0097'},durationMs:1000},sync:{state:'synced',complete:true,message:null,usageSyncedAt:'2026-10-02T03:54:22Z',balanceSyncedAt:'2026-10-02T03:54:22Z'}}};
        window.__probeState = state;
        window.__commands=[];
        mockIPC(async (cmd,args) => {
          window.__commands.push({cmd,args});
          if(cmd==='bootstrap')return state;
          if(cmd==='set_float_layout')return window.__resizeProbe(args);
          if(cmd==='save_settings'){state.settings={...args.settings};state.generation++;return state;}
          if(cmd==='reset_float_size'){const size=await window.__resetProbe();window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:float-size',payload:size});return null;}
          if(cmd==='query_records')return {accountId:accounts[0].id,generation:1,range:'today',items:[state.snapshot.latest],total:1,page:1,pageSize:20,complete:true};
          if(cmd==='analysis')return {accountId:accounts[0].id,generation:1,range:'today',start:state.snapshot.start,end:state.snapshot.end,timezone:'Asia/Shanghai',syncedAt:state.snapshot.end,trend:[{name:'08:00',totals},{name:'09:00',totals:{...totals,cost:'25.00'}},{name:'10:00',totals:{...totals,cost:'37.80'}}],models:[{name:'gpt-6.1-sol',totals}],keys:[{name:'工作电脑',totals}],complete:true};
          return null;
        }, {shouldMockEvents:true});
      `);
      await page.goto(url);
      await page.getByRole('button', { name: '展开', exact: true }).waitFor();
      await page.waitForTimeout(350);
      async function inspect(name, bounded = false) {
        await page.waitForTimeout(350);
        const result = await page.evaluate(() => {
          const html = document.documentElement, body = document.body, card = document.querySelector('.float-card'), footer = document.querySelector('.float-footer'), panel = document.querySelector('.float-expanded');
          const rect = card.getBoundingClientRect(), footerRect = footer.getBoundingClientRect();
          return {viewportWidth:innerWidth,viewportHeight:innerHeight,documentWidth:Math.max(html.scrollWidth,body.scrollWidth),documentHeight:Math.max(html.scrollHeight,body.scrollHeight),cardBottom:rect.bottom,footerBottom:footerRect.bottom,bodyOverflow:getComputedStyle(body).overflow,panelClientHeight:panel?.clientHeight,panelScrollHeight:panel?.scrollHeight};
        });
        const panelFits = result.panelClientHeight == null || (bounded ? result.panelScrollHeight > result.panelClientHeight : result.panelScrollHeight <= result.panelClientHeight);
        const passed = result.documentWidth <= result.viewportWidth && result.documentHeight <= result.viewportHeight && result.cardBottom <= result.viewportHeight && result.footerBottom <= result.viewportHeight && panelFits;
        checks.push({name,scale,passed,...result});
        return result;
      }
      await inspect('compact');
      await page.getByRole('button', { name: '展开', exact: true }).click();
      await inspect('expanded');
      await page.evaluate(() => {
        window.__probeLatest = window.__probeState.snapshot.latest;
        window.__probeState.snapshot.latest = null;
        window.__probeState.selectedRange = 'recent';
        window.__probeState.snapshot.range = 'recent';
        window.__probeState.generation++;
        window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});
      });
      await inspect('expanded-shorter-live-content');
      await page.evaluate(() => {
        window.__probeState.snapshot.latest = window.__probeLatest;
        window.__probeState.selectedRange = 'today';
        window.__probeState.snapshot.range = 'today';
        window.__probeState.generation++;
        window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});
      });
      await inspect('expanded-live-content-grows');
      await page.evaluate(() => {
        window.__probeState.accounts[0].preferences.metrics.push('balance');
        window.__probeState.generation++;
        window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});
      });
      await inspect('expanded-metric-row-added');
      await page.evaluate(() => {
        window.__probeState.accounts[0].preferences.metrics.pop();
        window.__probeState.generation++;
        window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});
      });
      await inspect('expanded-metric-row-removed');
      await page.getByRole('button', { name: '切换账号', exact: true }).click();
      await inspect('expanded-account-menu');
      await page.keyboard.press('Escape');
      await page.getByRole('button', { name: '收起', exact: true }).click();
      const compactAgain = await inspect('collapsed-again');
      await page.evaluate(() => {
        const sample = window.__probeState.accounts[1];
        window.__probeState.accounts.push(...Array.from({length:10},(_,i)=>({...sample,id:'extra-'+i,preferences:{...sample.preferences,alias:'示例账号 '+i}})));
        window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});
      });
      await page.getByRole('button', {name:'切换账号',exact:true}).click();
      const menuOpen = await inspect('compact-many-accounts-menu');
      checks.push({name:'menu-expands-window',scale,passed:menuOpen.viewportHeight>compactAgain.viewportHeight,compactHeight:compactAgain.viewportHeight,menuHeight:menuOpen.viewportHeight});
      const menu = await page.locator('.account-menu').evaluate(element=>{const rect=element.getBoundingClientRect();return {top:rect.top,bottom:rect.bottom,height:innerHeight};});
      checks.push({name:'menu-contained',scale,passed:menu.top>=0&&menu.bottom<=menu.height,...menu});
      await page.keyboard.press('Escape');
      const menuClosed = await inspect('many-accounts-menu-closed');
      checks.push({name:'menu-close-restores-window-height',scale,passed:menuClosed.viewportHeight===compactAgain.viewportHeight});
      maximumHeight = 330;
      await page.getByRole('button', { name: '展开', exact: true }).click();
      await inspect('short-work-area', true);
      await page.locator('.float-expanded').evaluate(panel => { panel.scrollTop = panel.scrollHeight; });
      await inspect('short-work-area-after-scroll', true);
      checks.push({name:'resize-settles',scale,passed:requests.length < 32,requests:requests.length});
      if (scale === 1.5) {
        await page.screenshot({path:path.join(reportDirectory,'float-layout-short.png')});
        maximumHeight = 1400;
        await page.getByRole('button', { name:'收起',exact:true }).click();
        await inspect('collapsed-before-work-area-restored');
        await page.getByRole('button', { name:'展开',exact:true }).click();
        await inspect('work-area-restored');
        await page.screenshot({path:path.join(reportDirectory,'float-layout-expanded.png')});
      }
      maximumHeight = 1400;
      if (await page.getByRole('button',{name:'收起',exact:true}).count()) await page.getByRole('button',{name:'收起',exact:true}).click();
      await inspect('before-manual-resize');
      async function setManualSize(nextWidth, nextHeight, resizing = false) {
        width=nextWidth; manualHeight=nextHeight;
        await page.evaluate(size => window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:float-size',payload:size}),{width,height:nextHeight,manualHeight:true,resizing});
        await page.setViewportSize({width,height:nextHeight});
      }
      await setManualSize(420,420);
      await inspect('manual-tall-compact');
      checks.push({name:'manual-card-fills-height',scale,passed:await page.locator('.float-card').evaluate(e=>Math.abs(e.getBoundingClientRect().bottom-(innerHeight-8))<1)});
      await setManualSize(280,300);
      await inspect('manual-narrow-compact');
      checks.push({name:'natural-minimum-independent-of-manual-fill',scale,passed:requests.at(-1).minHeight<300,last:requests.at(-1)});
      await page.evaluate(()=>{window.__probeState.snapshot.totals.cost='1234567890.12';window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});});
      await inspect('narrow-long-cost');
      checks.push({name:'long-numbers-fit',scale,passed:await page.locator('.metric-value').evaluateAll(es=>es.every(e=>e.scrollWidth<=e.clientWidth+1))});
      await page.evaluate(()=>{window.__probeState.snapshot.totals.cost='26.80';window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});});
      await page.getByRole('button',{name:'切换账号',exact:true}).click();
      checks.push({name:'resize-handles-disabled-during-menu',scale,passed:await page.locator('.resize-edge').count()===0});
      await inspect('narrow-menu');
      await page.keyboard.press('Escape');
      checks.push({name:'eight-resize-directions',scale,passed:await page.locator('.resize-edge').count()===8});
      await page.locator('[data-resize="SouthEast"]').dispatchEvent('pointerdown',{button:0});
      const beforeGesture=requests.length;
      await page.setViewportSize({width:400,height:420});
      await page.waitForTimeout(180);
      checks.push({name:'no-layout-write-during-native-resize',scale,passed:requests.length===beforeGesture});
      await setManualSize(400,420,false);
      await inspect('native-resize-complete');
      await page.getByRole('button',{name:'展开',exact:true}).click();
      await setManualSize(400,650);
      await inspect('manual-tall-expanded');
      const expandedMinimum=requests.at(-1).minHeight;
      await setManualSize(400,360);
      await inspect('manual-short-expanded',true);
      checks.push({name:'expanded-minimum-does-not-follow-stretched-height',scale,passed:requests.at(-1).minHeight===expandedMinimum});
      await page.getByRole('button',{name:'收起',exact:true}).click();
      await setManualSize(400,420);
      checks.push({name:'native-resize-command',scale,passed:await page.evaluate(()=>window.__commands.some(c=>c.cmd==='start_float_resize'&&c.args.direction==='SouthEast'))});
      await page.getByRole('button',{name:'隐藏到托盘',exact:true}).click();
      checks.push({name:'hide-command-is-not-close',scale,passed:await page.evaluate(()=>window.__commands.some(c=>c.cmd==='plugin:window|hide')&&!window.__commands.some(c=>c.cmd==='plugin:window|close'))});
      await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('reset_float_size'));
      await inspect('reset-default-size');
      layoutDelay=180;
      await page.getByRole('button',{name:'展开',exact:true}).click();
      await page.waitForTimeout(70);
      await page.getByRole('button',{name:'切换账号',exact:true}).click();
      await page.waitForTimeout(70);
      await page.keyboard.press('Escape');
      await page.getByRole('button',{name:'收起',exact:true}).click();
      await page.waitForTimeout(650);
      await inspect('slow-layout-latest-state');
      checks.push({name:'layout-commands-serialized',scale,passed:peakInFlightLayouts===1,peakInFlightLayouts});
      checks.push({name:'slow-layout-latest-mode-wins',scale,passed:!requests.at(-1).expanded&&!requests.at(-1).menuOpen,last:requests.at(-1)});
      layoutDelay=0;
      for (const theme of ['light','dark']) {
        await page.evaluate(theme=>{window.__probeState.settings.theme=theme;window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});},theme);
        await page.waitForTimeout(100);
        checks.push({name:`${theme}-theme-applied`,scale,passed:await page.locator('html').getAttribute('data-theme')===theme});
        if(scale===1) await page.screenshot({path:path.join(reportDirectory,`theme-${theme}-float.png`)});
      }
      await page.evaluate(()=>{window.__probeState.settings.theme='system';window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});});
      await page.emulateMedia({colorScheme:'light'});
      await page.waitForTimeout(80);
      checks.push({name:'system-light',scale,passed:await page.locator('html').getAttribute('data-theme')==='light'});
      const systemCommands=await page.evaluate(()=>window.__commands.filter(c=>c.cmd==='plugin:window|set_theme'));
      checks.push({name:'system-releases-native-theme-override',scale,passed:systemCommands.at(-1)?.args.value===null});
      await page.emulateMedia({colorScheme:'dark'});
      await page.waitForTimeout(80);
      checks.push({name:'system-dark-change',scale,passed:await page.locator('html').getAttribute('data-theme')==='dark'});
      checks.push({name:'system-media-change-does-not-force-native-theme',scale,passed:await page.evaluate(count=>{const commands=window.__commands.filter(c=>c.cmd==='plugin:window|set_theme');return commands.length===count&&commands.at(-1)?.args.value===null;},systemCommands.length)});
      await page.emulateMedia({colorScheme:'light'});
      await page.evaluate(()=>{window.__probeState.settings.theme='light';window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});});
      await page.waitForTimeout(80);
      await page.evaluate(()=>{window.__probeState.settings.theme='system';window.__probeState.generation++;window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'subgauge:state',payload:window.__probeState});});
      await page.waitForTimeout(80);
      checks.push({name:'same-effective-color-still-releases-native-override',scale,passed:await page.evaluate(()=>window.__commands.filter(c=>c.cmd==='plugin:window|set_theme').at(-1)?.args.value===null)});
      checks.push({name:'theme-titlebar-command',scale,passed:await page.evaluate(()=>window.__commands.some(c=>c.cmd==='plugin:window|set_theme'&&c.args.value==='dark'))});
      checks.push({name:'measurement-clones-cleaned',scale,passed:await page.locator('.layout-measure').count()===0});
      await page.setViewportSize({width:960,height:820});
      await page.goto(url+'?window=details&page=settings');
      await page.getByRole('combobox',{name:'外观主题',exact:true}).waitFor();
      await page.getByRole('button',{name:'恢复默认尺寸',exact:true}).click();
      checks.push({name:'reset-size-command',scale,passed:await page.evaluate(()=>window.__commands.some(c=>c.cmd==='reset_float_size'))});
      await page.setViewportSize({width:960,height:820});
      await page.getByRole('button',{name:'最小化详情窗口',exact:true}).click();
      checks.push({name:'minimize-details-command',scale,passed:await page.evaluate(()=>window.__commands.some(c=>c.cmd==='plugin:window|minimize'))});
      for (const theme of ['light','dark']) {
        await page.getByRole('combobox',{name:'外观主题',exact:true}).selectOption(theme);
        await page.getByRole('button',{name:'保存设置',exact:true}).click();
        await page.getByRole('button',{name:'用量概览',exact:true}).click();
        await page.waitForTimeout(150);
        checks.push({name:`${theme}-detail-theme`,scale,passed:await page.locator('html').getAttribute('data-theme')===theme});
        if(scale===1) { await page.evaluate(()=>window.scrollTo(0,0)); await page.screenshot({path:path.join(reportDirectory,`theme-${theme}-details.png`),fullPage:true}); }
        await page.getByRole('button',{name:'应用设置',exact:true}).click();
      }
      await context.close();
    }
    const report = {scope:'headless browser with official Tauri IPC mocks and sample data; does not replace native window or pointer acceptance',passed:checks.every(c=>c.passed),checks};
    fs.writeFileSync(reportPath,JSON.stringify(report,null,2));
    console.log(`Floating layout: ${checks.filter(c=>c.passed).length}/${checks.length} checks passed. Report: ${reportPath}`);
    assert(report.passed,'floating layout has overflow or fails to settle');
  } finally { await browser?.close(); await server.close(); }
})().catch(error => { console.error(error.message); process.exit(1); });
