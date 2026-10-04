const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

const repo = path.resolve(__dirname, '..');
const channel = process.env.SUBGAUGE_BROWSER_CHANNEL || 'msedge';
const reportPath = path.join(repo, 'release/validation/freshness-regression.json');
const fixtureId = '\0virtual:freshness-regression';
const fixture = `
  import {createApp,h,reactive} from 'vue';
  import Metrics from '/src/components/Metrics.vue';
  import {syncPresentation} from '/src/freshness.ts';
  import '/src/style.css';
  const now=Date.parse('2026-10-04T04:05:00Z');
  const totals={actualCost:1234.123456789,actualCostExact:'1234.123456789012345678',requests:1234567,
    inputTokens:1000000,outputTokens:2345,cacheReadTokens:9000000,cacheCreationTokens:100000};
  const fresh={accountId:'synthetic',generation:1,range:'today',timezone:'Asia/Shanghai',
    start:'2026-10-03T16:00:00Z',end:'2026-10-04T04:05:00Z',totals,recent:totals,latest:null,
    balance:10000.123456789,balanceExact:'10000.123456789012345678',
    balanceUpdatedAt:'2026-10-04T04:05:00Z',usageUpdatedAt:'2026-10-04T04:05:00Z',recentUpdatedAt:'2026-10-04T04:05:00Z',status:'synced'};
  const settings={summaryRefreshSeconds:30,recentRefreshSeconds:10};
  const state=reactive({fields:['cost','requests','tokens','cache','balance'],totals,balance:fresh.balance,balanceExact:fresh.balanceExact});
  window.__freshnessProbe={now,fresh,settings,state,syncPresentation};
  createApp({setup:()=>()=>h(Metrics,{fields:state.fields,totals:state.totals,balance:state.balance,balanceExact:state.balanceExact})}).mount('#metrics');
`;

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({
    root: repo,
    server: { host: '127.0.0.1', port: 0, strictPort: false, open: false },
    plugins: [{
      name: 'isolated-freshness-fixture',
      resolveId(id) { return id === fixtureId ? fixtureId : undefined; },
      load(id) { return id === fixtureId ? fixture : undefined; },
      configureServer(vite) {
        vite.middlewares.use(async (request, response, next) => {
          if (request.url !== '/__freshness_probe') return next();
          try {
            const html = await vite.transformIndexHtml('/__freshness_probe', '<!doctype html><html data-theme="light"><body><div id="metrics" style="width:300px"></div><script type="module" src="/@id/__x00__virtual:freshness-regression"></script></body></html>');
            response.setHeader('Content-Type', 'text/html; charset=utf-8');
            response.end(html);
          } catch (error) { next(error); }
        });
      },
    }],
  });
  let browser;
  const checks = [];
  const check = (name, passed) => { checks.push({ name, passed: !!passed }); assert(passed, name); };
  try {
    await server.listen();
    browser = await chromium.launch({ channel, headless: true });
    const page = await browser.newPage({ viewport: { width: 400, height: 600 } });
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/__freshness_probe`);
    await page.locator('.metric').first().waitFor();
    const helperChecks = await page.evaluate(() => {
      const { fresh, now, settings, syncPresentation } = window.__freshnessProbe;
      const checks = [];
      const add = (name, passed) => checks.push({ name, passed: !!passed });
      const result = (patch = {}, fields = ['cost'], elapsed = 0, intervals = settings) => syncPresentation({ ...fresh, ...patch }, intervals, fields, now + elapsed);
      const old = new Date(now - 91000).toISOString();
      add('fresh-complete-zero-safe-snapshot-is-synced', result().good && result().label === '已同步');
      add('freshness-minimum-90-seconds-includes-exact-boundary', result({}, ['cost'], 90000).good);
      add('time-alone-expires-previously-synced-data', !result({}, ['cost'], 90001).good && result({}, ['cost'], 90001).label === '数据未更新');
      add('summary-threshold-scales-to-three-refresh-intervals', result({}, ['cost'], 179999, { summaryRefreshSeconds: 60, recentRefreshSeconds: 10 }).good && !result({}, ['cost'], 180001, { summaryRefreshSeconds: 60, recentRefreshSeconds: 10 }).good);
      add('invalid-intervals-use-safe-defaults', result({}, ['cost'], 90000, { summaryRefreshSeconds: NaN, recentRefreshSeconds: -1 }).good && !result({}, ['cost'], 90001, { summaryRefreshSeconds: NaN, recentRefreshSeconds: -1 }).good);
      add('unselected-stale-balance-does-not-expire-usage', result({ balanceUpdatedAt: old }).good);
      add('selected-stale-balance-expires-state', result({ balanceUpdatedAt: old }, ['balance']).label === '数据未更新');
      add('selected-missing-balance-is-not-synced', !result({ balance: null }, ['balance']).good);
      add('missing-usage-time-is-not-synced', result({ usageUpdatedAt: null }).label === '尚未同步');
      add('missing-usage-totals-is-not-synced', !result({ totals: null }).good);
      add('balance-only-card-does-not-require-old-usage', result({ usageUpdatedAt: old, totals: null }, ['balance']).good);
      add('balance-only-recent-range-does-not-require-window', result({ range: 'recent', usageUpdatedAt: null, totals: null, recent: null, recentUpdatedAt: null }, ['balance']).good);
      add('invalid-usage-time-is-not-synced', !result({ usageUpdatedAt: 'not-a-date' }).good && result({ usageUpdatedAt: 'not-a-date' }).title.includes('时间不可用'));
      add('future-time-is-conservatively-unsynced', !result({ usageUpdatedAt: new Date(now + 1).toISOString() }).good && result({ usageUpdatedAt: new Date(now + 1).toISOString() }).title.includes('晚于本机时间'));
      add('unused-recent-age-does-not-expire-today', result({ recentUpdatedAt: old }).good);
      add('recent-range-requires-recent-time', !result({ range: 'recent', recentUpdatedAt: null }).good);
      add('recent-range-requires-recent-data', !result({ range: 'recent', recent: null }).good);
      add('recent-range-expires-on-recent-age', result({ range: 'recent', recentUpdatedAt: old }).label === '数据未更新');
      add('recent-range-uses-recent-refresh-interval', result({ range: 'recent' }, ['cost'], 100000, { summaryRefreshSeconds: 30, recentRefreshSeconds: 40 }).good && !result({ range: 'recent' }, ['cost'], 120001, { summaryRefreshSeconds: 30, recentRefreshSeconds: 40 }).good);
      for (const [status, label] of Object.entries({ needsLogin: '需重新登录', incomplete: '数据不完整', partial: '部分数据未更新', loading: '同步中', syncing: '同步中', error: '同步失败', offline: '离线', expired: '登录已过期', stale: '数据未更新' })) {
        const state = result({ status, usageUpdatedAt: old, message: '原始失败原因' });
        add(`native-${status}-is-never-overridden-by-aging`, !state.good && state.label === label && state.title.includes('原始失败原因'));
      }
      add('unknown-state-does-not-claim-synced', !result({ status: 'unexpected' }).good);
      add('tooltip-lists-all-three-separate-times', result().title.includes('用量：') && result().title.includes('余额：') && result().title.includes('最近窗口：'));
      add('tooltip-applies-account-timezone', result().title.includes('12:05:00') && syncPresentation(fresh, settings, ['cost'], now, 'UTC').title.includes('04:05:00'));
      add('invalid-timezone-falls-back-to-explicit-utc', syncPresentation(fresh, settings, ['cost'], now, 'invalid-zone').title.includes('2026-10-04T04:05:00.000Z'));
      add('tooltip-natural-aging-is-visible', result({}, ['cost'], 65000).title.includes('1 分 5 秒前'));
      add('absent-snapshot-does-not-claim-synced', !syncPresentation(null, settings, ['cost'], now).good);
      const zero = Object.fromEntries(Object.keys(fresh.totals).filter(key => key !== 'actualCostExact').map(key => [key, 0]));
      add('complete-empty-usage-is-still-fresh', result({ totals: zero, balance: 0 }, ['balance']).good);
      return checks;
    });
    for (const item of helperChecks) check(item.name, item.passed);

    const metric = name => page.locator('.metric').filter({ has: page.getByText(name, { exact: true }) });
    check('cost-tooltip-retains-full-server-decimal', await metric('实际扣费').getAttribute('title') === '实际扣费：$1,234.123456789012345678');
    check('balance-tooltip-retains-full-server-decimal', await metric('当前余额').getAttribute('title') === '当前余额：$10,000.123456789012345678');
    check('requests-tooltip-is-unabridged', await metric('请求').getAttribute('title') === '请求：1,234,567 次');
    const tokenTitle = await metric('Token').getAttribute('title');
    check('token-tooltip-includes-total-and-all-components', ['Token 合计：10,102,345', '普通输入：1,000,000', '输出：2,345', '缓存读取：9,000,000', '缓存写入：100,000'].every(text => tokenTitle.includes(text)));
    const cacheTitle = await metric('缓存率').getAttribute('title');
    check('cache-tooltip-explains-weighted-input-denominator', cacheTitle.includes('缓存率：89.1%') && cacheTitle.includes('缓存读取：9,000,000') && cacheTitle.includes('输入合计：10,100,000（普通输入 + 缓存读取 + 缓存写入）'));
    check('tooltip-does-not-expand-card-with-full-values', (await metric('Token').locator('strong').innerText()) === '10.10M' && (await metric('请求').locator('strong').innerText()) === '1.23M次');
    const originalHeight = (await page.locator('.metrics').boundingBox()).height;
    await page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; });
    check('tooltips-do-not-change-dark-theme-layout', (await page.locator('.metrics').boundingBox()).height === originalHeight);
    await page.evaluate(() => { window.__freshnessProbe.state.totals = { actualCost: 0, requests: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 }; window.__freshnessProbe.state.balance = 0; window.__freshnessProbe.state.balanceExact = '0'; });
    await page.waitForFunction(() => document.querySelector('.metric[title="实际扣费：$0"]'));
    check('zero-cache-denominator-is-explained-without-false-percent', (await metric('缓存率').getAttribute('title')).includes('缓存率：—（输入 Token 为零）') && await metric('缓存率').locator('strong').innerText() === '—');
    check('zero-balance-is-a-value-not-missing', await metric('当前余额').getAttribute('title') === '当前余额：$0');
    await page.evaluate(() => { window.__freshnessProbe.state.totals = null; window.__freshnessProbe.state.balance = null; window.__freshnessProbe.state.balanceExact = null; });
    await page.waitForFunction(() => [...document.querySelectorAll('.metric')].every(element => element.title.endsWith('尚未同步')));
    check('missing-values-show-no-synthetic-zero', (await page.locator('.metric strong').allTextContents()).every(value => value === '—'));
    await page.evaluate(() => { window.__freshnessProbe.state.totals = { actualCost: 1.25, actualCostExact: '<img src=x>', requests: 1, inputTokens: 1, outputTokens: 2, cacheReadTokens: 3, cacheCreationTokens: 4 }; window.__freshnessProbe.state.balance = -12.5; window.__freshnessProbe.state.balanceExact = '-12.500000'; });
    await page.waitForFunction(() => document.querySelector('.metric[title="实际扣费：$1.25"]'));
    check('invalid-exact-decimal-falls-back-and-renders-no-markup', await metric('实际扣费').getAttribute('title') === '实际扣费：$1.25' && await page.locator('img').count() === 0);
    check('negative-balance-keeps-server-precision', await metric('当前余额').getAttribute('title') === '当前余额：$-12.500000');

    const report = { scope: `Real freshness helper and Vue Metrics component in isolated headless browser (${channel}); synthetic data only, no account or native desktop access`, passed: checks.every(item => item.passed), checks };
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { try { await browser?.close(); } finally { await server.close(); } }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
