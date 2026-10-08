const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const repo = path.resolve(__dirname, '..');
const mocks = fs.readFileSync(path.join(repo, 'node_modules/@tauri-apps/api/mocks.js'), 'utf8').replace(/^export .*$/gm, '');
const channel = process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge';
const reportPath = path.join(repo, 'release/validation/polish-regression.json');
const fixtureId = '\0virtual:polish-regression';
const helperFixture = `
  import { money, relativeTime } from '/src/format.ts';
  import { trendBuckets } from '/src/trend.ts';
  import { syncPresentation } from '/src/freshness.ts';
  import { demoApi } from '/src/demo.ts';
  window.__polishHelpers = { money, relativeTime, trendBuckets, syncPresentation, demoApi };
`;
const initScript = mocks + `
  mockWindows('details');
  const stamp = new Date().toISOString();
  const totals = {cost:'12.50',requests:40,inputTokens:1000,outputTokens:200,cacheReadTokens:5000,cacheCreationTokens:20};
  const part = (state='synced',complete=true,message=null) => ({state,complete,message,syncedAt:stamp});
  const accounts = ['A','B'].map(id => ({id,site:'https://'+id.toLowerCase()+'.example.com',email:'example@example.com',role:'user',needsLogin:false,demo:true,
    preferences:{alias:'示例账号 '+id,defaultRange:'today',metrics:['cost','requests','tokens','cache'],recentMinutes:5,timezone:'Asia/Shanghai'}}));
  function makeSnapshot(accountId,generation) {
    return {accountId,generation,range:'today',timezone:'Asia/Shanghai',start:stamp,end:stamp,totals,recent:totals,latest:null,balance:'80.00',
      sync:{state:'synced',complete:true,message:null,usageSyncedAt:stamp,balanceSyncedAt:stamp,recentSyncedAt:stamp,usage:part(),balance:part(),recent:part()}};
  }
  const state = {generation:1,currentAccountId:'A',selectedRange:'today',accounts,
    settings:{theme:'light',opacity:1,alwaysOnTop:true,recentRefreshSeconds:10,summaryRefreshSeconds:30,backgroundRefreshSeconds:120},snapshot:makeSnapshot('A',1)};
  window.__polishProbe = {state,queries:[],optionQueries:[],heldOptions:[],releasedOptions:0,settingsCalls:[],optionsAgeMs:0,optionsComplete:true,holdOptionsA:location.search.includes('hold-options'),failOptions:location.search.includes('fail-options')};
  if(location.search.includes('partial-recent')) {
    state.snapshot.sync.complete=false;
    state.snapshot.sync.message='最近窗口记录待补齐';
    state.snapshot.sync.recent=part('incomplete',false,'示例分页仍在变化');
  }
  mockIPC(async (command,args) => {
    const probe=window.__polishProbe;
    if(command==='bootstrap')return structuredClone(state);
    if(command==='switch_account') {
      state.currentAccountId=args.id;state.generation++;state.snapshot=makeSnapshot(args.id,state.generation);
      return structuredClone(state);
    }
    if(command==='query_records') {
      const query=structuredClone(args.query);probe.queries.push(query);
      return {accountId:query.accountId,generation:state.generation,range:query.range,start:stamp,end:stamp,timezone:'Asia/Shanghai',page:query.page,pageSize:query.pageSize,total:1,complete:true,
        items:[{id:1,createdAt:stamp,model:query.model||'example-model',apiKeyId:query.apiKeyId||10,apiKeyName:query.apiKeyId===20?'主要工作 Key':'日常 Key',
          totals:{...totals,cost:'0.000123456789012345678',requests:1},durationMs:1200}]};
    }
    if(command==='filter_options') {
      const query=structuredClone(args.query),generation=state.generation;probe.optionQueries.push(query);
      if(probe.failOptions)throw new Error('示例候选服务暂不可用');
      if(probe.holdOptionsA && query.accountId==='A') {
        await new Promise(resolve=>probe.heldOptions.push(resolve));
        requestAnimationFrame(()=>{probe.releasedOptions++;});
      }
      return {accountId:query.accountId,generation,range:query.range,start:stamp,end:stamp,timezone:'Asia/Shanghai',syncedAt:new Date(Date.now()-probe.optionsAgeMs).toISOString(),
        models:query.accountId==='A'?['example-model','other-model']:['only-B-model'],
        keys:query.accountId==='A'?[{id:10,name:'日常 Key'},{id:20,name:'主要工作 Key'}]:[{id:70,name:'仅 B 的 Key'}],modelsComplete:probe.optionsComplete,keysComplete:probe.optionsComplete};
    }
    if(command==='analysis') {
      const row=(name,cost,keyId)=>({name,keyId,totals:{...totals,cost}});
      return {accountId:args.query.accountId,generation:state.generation,range:args.query.range,start:stamp,end:stamp,timezone:'Asia/Shanghai',syncedAt:stamp,trend:[],complete:true,
        models:[row('低消耗模型','1'),row('高消耗模型','8')],keys:[row('日常 Key','2',10),row('主要工作 Key','9',20)]};
    }
    if(command==='patch_settings') {
      probe.settingsCalls.push(structuredClone(args));Object.assign(state.settings,args.patch);state.generation++;return structuredClone(state);
    }
    if(command==='desktop_status')return {revision:1,distribution:'installed',launchAtLogin:false,startupVisibility:'float',shortcut:null,shortcutRegistered:false,startupBlocked:null,error:null};
    if(command==='update_status')return {revision:1,currentVersion:'0.1.9',distribution:'installed',channel:'preview',phase:'upToDate',autoCheck:false,checkedAt:stamp,version:null,notes:null,publishedAt:null,downloadedBytes:0,totalBytes:null,error:null,skippedVersion:null};
    if(command==='set_float_layout')return {width:316,height:args.height,manualHeight:false,resizing:false};
    return null;
  },{shouldMockEvents:true});
`;

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({
    root: repo, server: { host: '127.0.0.1', port: 0, strictPort: false, open: false },
    plugins: [{
      name: 'isolated-polish-helpers',
      resolveId(id) { return id === fixtureId ? fixtureId : undefined; },
      load(id) { return id === fixtureId ? helperFixture : undefined; },
      configureServer(vite) {
        vite.middlewares.use(async (request, response, next) => {
          if (request.url !== '/__polish_helpers') return next();
          try {
            const html = await vite.transformIndexHtml('/__polish_helpers', '<!doctype html><html><body><script type="module" src="/@id/__x00__virtual:polish-regression"></script></body></html>');
            response.setHeader('Content-Type', 'text/html; charset=utf-8');response.end(html);
          } catch (error) { next(error); }
        });
      },
    }],
  });
  let browser, failure;
  const checks = [];
  const check = (name, passed) => { checks.push({ name, passed: !!passed }); assert(passed, name); };
  try {
    await server.listen();
    browser = await chromium.launch({ channel, headless: true });
    const baseUrl = `http://127.0.0.1:${server.httpServer.address().port}`;
    const helpers = await browser.newPage();
    await helpers.goto(baseUrl + '/__polish_helpers');
    await helpers.waitForFunction(() => !!window.__polishHelpers);
    const helperChecks = await helpers.evaluate(() => {
      const { money, relativeTime, trendBuckets, syncPresentation } = window.__polishHelpers;
      const results = [], add = (name, passed) => results.push({ name, passed: !!passed });
      add('small-fees-remain-visible', money(.0002) === '$0.0002' && money(.0099) === '$0.0099');
      add('extremely-small-fees-use-a-bound-instead-of-zero', money(.0000001) === '<$0.0001' && money(.0000001, 4) === '<$0.0001');
      add('zero-and-negative-money-are-not-missing', money(0) === '$0.00' && money(-.0002) === '$-0.0002' && money(-12.5) === '$-12.50' && money(-.0000001) === '>-$0.0001');
      add('money-rejects-missing-or-nonfinite-values', [null, undefined, NaN, Infinity, -Infinity].every(value => money(value) === '—'));
      const now = Date.parse('2026-10-08T00:00:30Z');
      add('relative-time-crosses-calendar-midnight-by-elapsed-time', relativeTime('2026-10-07T23:58:30Z', now) === '2 分钟前' && relativeTime('2026-10-06T23:00:30Z', now) === '1 天前');
      add('relative-time-does-not-label-future-as-just-now', relativeTime('2026-10-08T00:00:31Z', now) === '时间待核对' && relativeTime('bad-time', now) === '时间不可用');
      const points = [{label:'2026-10-08 01:00',actualCost:1},{label:'2026-10-08 08:00',actualCost:2},{label:'2026-10-08 09:00',actualCost:3}];
      const meta = {source:'server',granularity:'hour',timezone:null,missingBuckets:'unknown',complete:true};
      const unknown = trendBuckets(points, meta);
      add('nonconsecutive-hours-retain-elapsed-spacing-and-unknown-gaps', unknown.length === 9 && unknown[0].label === '01:00' && unknown[7].label === '08:00' && unknown.slice(1,7).every(point => point.actualCost === null));
      const known = trendBuckets(points, {...meta, source:'records', missingBuckets:'zero'});
      add('complete-records-explicitly-allow-zero-gap-buckets', known.length === 9 && known.slice(1,7).every(point => point.actualCost === 0));
      add('incomplete-records-never-invent-zero-gap-buckets', trendBuckets(points, {...meta,source:'records',missingBuckets:'zero',complete:false}).slice(1,7).every(point => point.actualCost === null));
      const duplicates = trendBuckets([points[0],points[0],points[2]],meta);
      add('duplicate-buckets-are-not-silently-merged-or-expanded', duplicates.length === 3 && duplicates.every(point => point.actualCost !== null));
      const invalid = trendBuckets([{label:'2026-02-30 01:00',actualCost:1},points[2]],meta);
      add('invalid-calendar-labels-are-preserved-without-fabricated-hours', invalid.length === 2 && invalid[0].label === '2026-02-30 01:00');
      const mixed = trendBuckets([{...points[0],bucketStart:'2026-10-07T17:00:00Z'},points[2]],{...meta,timezone:'Asia/Shanghai'});
      add('mixed-offset-and-calendar-labels-are-not-conflated', mixed.length === 2);
      const timestamped = trendBuckets([{label:'raw',bucketStart:'2026-10-07T17:00:00Z',actualCost:1},{label:'raw2',bucketStart:'2026-10-07T19:00:00Z',actualCost:2}],{...meta,timezone:'Asia/Shanghai'});
      add('explicit-timestamps-use-the-account-timezone', timestamped.map(point=>point.label).join(',') === '01:00,02:00,03:00');
      const invalidExplicit = trendBuckets([{...points[0],bucketStart:'invalid-time'},{...points[2],bucketStart:'2026-10-08T01:00:00Z'}],{...meta,timezone:'Asia/Shanghai'});
      add('invalid-explicit-bucket-timestamp-does-not-fall-back-to-calendar-coordinate', invalidExplicit.length === 2 && invalidExplicit[0].label === points[0].label);
      const invalidTimezone = trendBuckets([{...points[0],bucketStart:'2026-10-07T17:00:00Z'},{...points[2],bucketStart:'2026-10-08T01:00:00Z'}],{...meta,timezone:'invalid-zone'});
      add('invalid-trend-timezone-preserves-source-without-throwing', invalidTimezone.length === 2 && invalidTimezone[0].label === points[0].label);
      const dstDaily = trendBuckets([{label:'2026-03-07',bucketStart:'2026-03-07T05:00:00Z',actualCost:1},{label:'2026-03-09',bucketStart:'2026-03-09T04:00:00Z',actualCost:2}],{source:'records',granularity:'day',timezone:'America/New_York',missingBuckets:'zero',complete:true});
      add('daily-buckets-cross-spring-dst-by-calendar-date', dstDaily.map(point=>point.label).join(',') === '03-07,03-08,03-09' && dstDaily[1].actualCost === 0);
      const fallDaily = trendBuckets([{label:'2026-10-31',bucketStart:'2026-10-31T04:00:00Z',actualCost:1},{label:'2026-11-02',bucketStart:'2026-11-02T05:00:00Z',actualCost:2}],{source:'records',granularity:'day',timezone:'America/New_York',missingBuckets:'zero',complete:true});
      add('daily-buckets-cross-fall-dst-without-duplicating-a-date', fallDaily.map(point=>point.label).join(',') === '10-31,11-01,11-02' && fallDaily[1].actualCost === 0);
      const totals = {actualCost:1,requests:1,inputTokens:1,outputTokens:1,cacheReadTokens:1,cacheCreationTokens:0};
      const stamp = new Date(now).toISOString(), complete = {state:'synced',complete:true,message:null,syncedAt:stamp};
      const snapshot = {accountId:'synthetic',generation:1,range:'today',timezone:'Asia/Shanghai',start:stamp,end:stamp,totals,recent:totals,latest:null,balance:10,
        usageUpdatedAt:stamp,balanceUpdatedAt:stamp,recentUpdatedAt:stamp,status:'incomplete',message:'最近窗口待补齐',
        sync:{usage:complete,balance:complete,recent:{...complete,state:'incomplete',complete:false,message:'最近窗口示例分页变化'}}};
      const settings = {summaryRefreshSeconds:30,recentRefreshSeconds:10};
      const compact = syncPresentation(snapshot,settings,['cost'],now,'Asia/Shanghai',false);
      const expanded = syncPresentation(snapshot,settings,['cost'],now,'Asia/Shanghai',true);
      add('recent-incompleteness-does-not-contaminate-collapsed-today', compact.good && compact.label === '已同步');
      add('expanded-card-identifies-the-incomplete-recent-window', !expanded.good && expanded.label === '最近窗口待补齐' && expanded.title.includes('最近窗口示例分页变化'));
      add('recent-range-still-requires-complete-recent-data', !syncPresentation({...snapshot,range:'recent'},settings,['cost'],now,'Asia/Shanghai',false).good);
      const balanceOnly = syncPresentation({...snapshot,range:'recent'},settings,['balance'],now,'Asia/Shanghai',false);
      add('balance-only-collapsed-card-ignores-incomplete-recent-range', balanceOnly.good && balanceOnly.label === '已同步');
      const latestFailed = {...snapshot,sync:{...snapshot.sync,recent:complete,latest:{...complete,state:'error',complete:false,message:'最近请求查询失败'}}};
      add('latest-query-failure-does-not-contaminate-collapsed-summary', syncPresentation(latestFailed,settings,['cost'],now,'Asia/Shanghai',false).good);
      const latestExpanded = syncPresentation(latestFailed,settings,['cost'],now,'Asia/Shanghai',true);
      add('latest-query-failure-is-specific-when-expanded', !latestExpanded.good && latestExpanded.label === '最近一笔同步失败' && latestExpanded.title.includes('最近请求查询失败'));
      add('confirmed-empty-latest-is-not-a-sync-failure', syncPresentation({...snapshot,sync:{...snapshot.sync,recent:complete,latest:complete}},settings,['cost'],now,'Asia/Shanghai',true).good);
      add('stale-latest-query-is-distinct-from-fresh-recent-window', syncPresentation({...latestFailed,sync:{...latestFailed.sync,latest:{...complete,syncedAt:new Date(now-91000).toISOString()}}},settings,['cost'],now,'Asia/Shanghai',true).label === '最近一笔待更新');
      return results;
    });
    for (const item of helperChecks) check(item.name, item.passed);
    check('demo-snapshot-supplies-all-four-sync-parts-with-legacy-times', await helpers.evaluate(async () => {
      const { snapshot } = await window.__polishHelpers.demoApi.bootstrap();
      return ['usage','balance','recent','latest'].every(name => {
        const part=snapshot.sync[name];
        return part?.state === 'synced' && part.complete && part.syncedAt === snapshot.end;
      }) && snapshot.status === 'ready' && snapshot.usageUpdatedAt === snapshot.end && snapshot.balanceUpdatedAt === snapshot.end && snapshot.recentUpdatedAt === snapshot.end;
    }));
    await helpers.close();

    const page = await browser.newPage({ viewport: { width: 960, height: 760 } });
    await page.addInitScript(initScript);
    await page.goto(baseUrl + '/?window=details&page=records');
    const model = page.getByRole('combobox', { name: '模型', exact: true });
    const key = page.getByRole('combobox', { name: 'Key', exact: true });
    await page.locator('.request-row').waitFor();
    await key.fill('主要');
    await page.getByRole('option', { name: /主要工作 Key/ }).click();
    check('key-selection-displays-the-name', await key.inputValue() === '主要工作 Key');
    await page.getByRole('button', { name: '筛选', exact: true }).click();
    await page.waitForFunction(() => window.__polishProbe.queries.at(-1)?.apiKeyId === 20);
    check('key-name-selection-submits-the-stable-id', await page.evaluate(() => window.__polishProbe.queries.at(-1).apiKeyId === 20));
    await key.fill('discard-this-query');await key.press('Escape');
    check('escape-keeps-the-previously-selected-key', await key.inputValue() === '主要工作 Key');
    await key.fill('30');await key.press('Enter');
    await page.getByRole('button', { name: '筛选', exact: true }).click();
    await page.waitForFunction(() => window.__polishProbe.queries.at(-1)?.apiKeyId === 30);
    check('manual-valid-key-id-remains-supported', await key.inputValue() === '30');
    await page.getByRole('button', { name: '清除Key筛选', exact: true }).focus();
    await page.getByRole('button', { name: '清除Key筛选', exact: true }).press('Enter');
    check('keyboard-clear-removes-the-key-without-reopening-menu', await key.inputValue() === '' && await key.getAttribute('aria-expanded') === 'false');
    await model.fill('other-model');await model.press('Enter');
    const callsBeforeTyping = await page.evaluate(() => window.__polishProbe.queries.length);
    await model.fill('unconfirmed-query');await model.press('Tab');
    check('search-text-does-not-apply-a-filter-or-submit-the-form', await model.inputValue() === 'other-model' && await page.evaluate(count => window.__polishProbe.queries.length === count, callsBeforeTyping));
    await model.fill('');await model.press('ArrowUp');await model.press('Enter');
    check('keyboard-list-navigation-confirms-a-candidate', await model.inputValue() === 'example-model' && await model.getAttribute('aria-expanded') === 'false');
    await page.getByRole('button', { name: '重置', exact: true }).click();
    await page.waitForFunction(() => {const query=window.__polishProbe.queries.at(-1);return query?.apiKeyId === null && query.model === null;});

    await page.locator('.request-row .model-name').click();
    check('request-row-click-opens-full-detail', await page.getByRole('button', { name: '收起请求 1', exact: true }).count() === 1 && (await page.locator('.request-detail').innerText()).includes('$0.000123456789012345678'));
    await page.getByRole('button', { name: '收起请求 1', exact: true }).click();
    check('request-button-does-not-toggle-twice-through-row-bubbling', await page.locator('.request-detail').count() === 0);
    await page.getByRole('button', { name: '展开请求 1', exact: true }).focus();
    await page.getByRole('button', { name: '展开请求 1', exact: true }).press('Enter');
    check('request-details-remain-keyboard-accessible', await page.locator('.request-detail').count() === 1);
    await page.evaluate(() => {
      const cell=document.querySelector('.request-row .numeric'), selection=getSelection(), range=document.createRange();
      range.selectNodeContents(cell);selection.removeAllRanges();selection.addRange(range);cell.click();
    });
    check('selecting-request-numbers-does-not-toggle-details', await page.locator('.request-detail').count() === 1);
    await page.evaluate(() => getSelection().removeAllRanges());

    await page.getByRole('button', { name: '用量分析', exact: true }).click();
    await page.getByRole('button', { name: '查看主要工作 Key的请求', exact: true }).waitFor();
    const groups = page.locator('.analysis-columns>section');
    check('analysis-sorts-models-and-keys-by-descending-cost', await groups.nth(0).locator('.rank-row').first().getAttribute('aria-label') === '查看高消耗模型的请求' && await groups.nth(1).locator('.rank-row').first().getAttribute('aria-label') === '查看主要工作 Key的请求');
    await page.getByRole('button', { name: '查看主要工作 Key的请求', exact: true }).click();
    await page.getByRole('heading', { name: '请求记录', exact: true }).waitFor();
    await page.waitForFunction(() => window.__polishProbe.queries.at(-1)?.apiKeyId === 20);
    check('analysis-key-link-opens-filtered-records-by-id', await key.inputValue() === '主要工作 Key' && await page.evaluate(() => window.__polishProbe.queries.at(-1).model === null));
    check('record-filter-does-not-replace-all-key-summary', (await page.locator('.records-summary').innerText()).includes('$12.50') && (await page.locator('.filter-scope').innerText()).includes('全部 Key 汇总'));

    await page.getByRole('button', { name: '应用设置', exact: true }).click();
    await page.getByRole('combobox', { name: '外观主题', exact: true }).selectOption('dark');
    await page.getByRole('button', { name: '保存设置', exact: true }).click();
    const success = page.getByRole('status').filter({ hasText: '应用设置已保存' });
    await success.waitFor();
    check('settings-success-feedback-is-visible-in-the-current-viewport', await success.evaluate(element => {const rect=element.getBoundingClientRect();return rect.top>=0 && rect.bottom<=innerHeight && rect.left>=0 && rect.right<=innerWidth;}));
    check('theme-save-actually-applies-dark-theme', await page.locator('html').getAttribute('data-theme') === 'dark');
    await page.close();

    const candidates = await browser.newPage({ viewport: { width: 960, height: 760 } });
    await candidates.addInitScript(initScript);
    await candidates.goto(baseUrl + '/?window=details&page=overview');
    await candidates.locator('.request-row').waitFor();
    await candidates.evaluate(() => { window.__polishProbe.optionsAgeMs=301000; });
    await candidates.getByRole('button', { name: '请求记录', exact: true }).click();
    await candidates.waitForFunction(() => window.__polishProbe.optionQueries.length > 0 && document.querySelector('[role=combobox][aria-busy=false]'));
    const expiredCount = await candidates.evaluate(() => window.__polishProbe.optionQueries.length);
    await candidates.getByRole('button', { name: '用量概览', exact: true }).click();
    await candidates.getByRole('button', { name: '请求记录', exact: true }).click();
    await candidates.waitForFunction(count => window.__polishProbe.optionQueries.length > count, expiredCount);
    check('expired-five-minute-candidates-are-reloaded-on-reentry', await candidates.evaluate(count => window.__polishProbe.optionQueries.length > count, expiredCount));
    await candidates.getByRole('button', { name: '用量概览', exact: true }).click();
    await candidates.evaluate(() => { window.__polishProbe.optionsAgeMs=0;window.__polishProbe.optionsComplete=false; });
    await candidates.getByRole('button', { name: '请求记录', exact: true }).click();
    await candidates.getByText('候选说明', { exact: true }).waitFor();
    const incompleteCount = await candidates.evaluate(() => window.__polishProbe.optionQueries.length);
    await candidates.getByRole('button', { name: '用量概览', exact: true }).click();
    await candidates.getByRole('button', { name: '请求记录', exact: true }).click();
    await candidates.waitForFunction(count => window.__polishProbe.optionQueries.length > count, incompleteCount);
    check('incomplete-candidates-are-retried-on-reentry', await candidates.evaluate(count => window.__polishProbe.optionQueries.length > count, incompleteCount));
    await candidates.close();

    const race = await browser.newPage({ viewport: { width: 960, height: 760 } });
    await race.addInitScript(initScript);
    await race.goto(baseUrl + '/?window=details&page=records&hold-options=1');
    await race.waitForFunction(() => window.__polishProbe.heldOptions.length > 0);
    await race.getByRole('button', { name: '切换账号', exact: true }).click();
    await race.locator('.account-option').filter({ hasText: '示例账号 B' }).click();
    await race.getByRole('combobox', { name: 'Key', exact: true }).click();
    await race.getByRole('option', { name: /仅 B 的 Key/ }).waitFor();
    await race.evaluate(() => {for(const release of window.__polishProbe.heldOptions)release();});
    await race.waitForFunction(() => window.__polishProbe.releasedOptions === window.__polishProbe.heldOptions.length);
    check('late-account-a-candidates-cannot-overwrite-account-b', await race.getByRole('option', { name: /仅 B 的 Key/ }).count() === 1 && await race.getByRole('option', { name: /主要工作 Key/ }).count() === 0);
    await race.close();

    const failed = await browser.newPage({ viewport: { width: 960, height: 760 } });
    await failed.addInitScript(initScript);
    await failed.goto(baseUrl + '/?window=details&page=records&fail-options=1');
    await failed.getByText('候选说明', { exact: true }).waitFor();
    const fallbackModel = failed.getByRole('combobox', { name: '模型', exact: true });
    await fallbackModel.fill('precise-custom-model');await fallbackModel.press('Enter');
    await failed.getByRole('button', { name: '筛选', exact: true }).click();
    await failed.waitForFunction(() => window.__polishProbe.queries.at(-1)?.model === 'precise-custom-model');
    check('failed-candidates-do-not-disable-exact-model-filtering', (await failed.locator('.request-row .model-name').innerText()) === 'precise-custom-model');
    await failed.close();

    const float = await browser.newPage({ viewport: { width: 316, height: 800 } });
    await float.addInitScript(initScript);
    await float.goto(baseUrl + '/?partial-recent=1');
    await float.locator('.float-footer .sync-status').waitFor();
    check('real-float-view-keeps-today-synced-when-only-recent-is-incomplete', (await float.locator('.float-footer .sync-status').innerText()).includes('已同步') && await float.locator('.compact-error').count() === 0);
    await float.getByRole('button', { name: '展开', exact: true }).click();
    check('expanded-float-identifies-which-window-needs-completion', (await float.locator('.float-footer .sync-status').innerText()).includes('最近窗口待补齐') && await float.locator('.sync-explanation').count() === 1);
    await float.getByRole('button', { name: '收起', exact: true }).click();
    check('collapsing-again-restores-the-relevant-status', (await float.locator('.float-footer .sync-status').innerText()).includes('已同步'));
    await float.close();
  } catch (error) { failure = error; }
  finally {
    try { await browser?.close(); } finally { await server.close(); }
    const report = { scope: `Real Vue app and helpers, official Tauri mocks, isolated headless browser (${channel}); synthetic data only, no native input or account access`, passed: !failure && checks.every(item => item.passed), checks, ...(failure ? { error: failure.message } : {}) };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  }
  if (failure) throw failure;
})().catch(error => { console.error(error.message); process.exitCode = 1; });
