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
    await page.addInitScript(mocks + `
      mockWindows('details');
      const totals = {cost:'1.25',requests:1,inputTokens:100,outputTokens:30,cacheReadTokens:50,cacheCreationTokens:0};
      const state = {
        generation:1,currentAccountId:'synthetic',selectedRange:'today',
        accounts:[{id:'synthetic',site:'https://example.com',email:'demo@example.com',role:'user',needsLogin:false,demo:true,preferences:{alias:'隔离示例',defaultRange:'today',metrics:['cost','requests','tokens','cache'],recentMinutes:5,timezone:'Asia/Shanghai'}}],
        settings:{theme:'light',opacity:1,alwaysOnTop:true,recentRefreshSeconds:10,summaryRefreshSeconds:30,backgroundRefreshSeconds:120},
        snapshot:{accountId:'synthetic',generation:1,range:'today',timezone:'Asia/Shanghai',start:'2026-10-02T00:00:00Z',end:'2026-10-02T01:00:00Z',totals,recent:totals,latest:null,balance:'10.00',sync:{state:'synced',complete:true,message:null,usageSyncedAt:'2026-10-02T01:00:00Z',balanceSyncedAt:'2026-10-02T01:00:00Z'}}
      };
      window.__detailProbe={overviewQueries:[],failOverview:true,slowStarted:false,slowFinished:false,saves:0,savedAlias:null};
      mockIPC(async (command,args) => {
        if(command==='bootstrap')return state;
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
        return null;
      },{shouldMockEvents:true});
    `);
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
    const report = { scope: `Real Vue app, official Tauri mocks, synthetic data, isolated headless browser (${channel})`, passed: checks.every(check => check.passed), checks };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
    assert(report.passed, 'Detail regression checks failed');
  } finally { try { await browser?.close(); } finally { await server.close(); } }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
