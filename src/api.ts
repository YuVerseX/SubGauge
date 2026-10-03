import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { AccountPreferences, AccountSummary, AppSettings, AppSettingsPatch, Bootstrap, FloatSizeState, LoginInput, LoginResult, QueryInput, ResizeDirection, UsageAnalysis, UsagePage, UsageRange, UsageRecord, UsageSnapshot, UsageTotals } from './types'
import { demoApi } from './demo'

export const isNative = '__TAURI_INTERNALS__' in window
export const isDemo = !isNative && new URLSearchParams(location.search).get('demo') === '1'
type NativeTotals = { cost: string; requests: number; inputTokens: number; outputTokens: number; cacheReadTokens: number; cacheCreationTokens: number }
type NativeAccount = { id: string; site: string; email: string; role: string; preferences: AccountPreferences & { alias: string }; needsLogin: boolean; demo: boolean }
type NativeRecord = { id: number; createdAt: string; model: string; apiKeyId: number; apiKeyName: string; totals: NativeTotals; durationMs?: number }
type NativeSnapshot = { accountId: string; generation: number; range: UsageRange; timezone: string; start: string; end: string; totals: NativeTotals | null; recent: NativeTotals | null; latest: NativeRecord | null; balance: string | null; sync: { state: string; message: string | null; usageSyncedAt: string | null; balanceSyncedAt: string | null; complete: boolean } }
type NativeState = { accounts: NativeAccount[]; currentAccountId: string | null; selectedRange: UsageRange; settings: AppSettings; snapshot: NativeSnapshot | null; generation: number }
const totals = (t: NativeTotals): UsageTotals => ({ ...t, actualCost: Number(t.cost) })
const record = (r: NativeRecord): UsageRecord => ({ ...totals(r.totals), id: String(r.id), createdAt: r.createdAt, model: r.model, keyId: String(r.apiKeyId), keyName: r.apiKeyName, durationMs: r.durationMs })
const account = (a: NativeAccount): AccountSummary => ({ id: a.id, alias: a.preferences.alias || a.email, siteUrl: a.site, email: a.email, role: a.role, preferences: a.preferences, sessionStatus: a.needsLogin ? 'needsLogin' : 'ready', demo: a.demo })
function snapshot(s: NativeSnapshot): UsageSnapshot { return { accountId: s.accountId, generation: s.generation, range: s.range, timezone: s.timezone, start: s.start, end: s.end, totals: s.totals && totals(s.totals), recent: s.recent && totals(s.recent), latest: s.latest && record(s.latest), balance: s.balance == null ? null : Number(s.balance), status: !s.sync.complete && ['ready', 'synced'].includes(s.sync.state) ? 'incomplete' : s.sync.state, message: s.sync.message, usageUpdatedAt: s.sync.usageSyncedAt, balanceUpdatedAt: s.sync.balanceSyncedAt } }
function state(s: NativeState): Bootstrap { return { generation: s.generation, accounts: s.accounts.map(account), activeAccountId: s.currentAccountId, settings: { ...s.settings, theme: s.settings.theme || 'light' }, range: s.selectedRange, snapshot: s.snapshot && snapshot(s.snapshot) } }
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> { if (!isNative) throw new Error('请在 SubGauge 桌面应用中连接账号。浏览器仅支持显式示例预览。'); return invoke<T>(command, args) }
export const api = {
  async bootstrap(): Promise<Bootstrap> { return isDemo ? demoApi.bootstrap() : state(await call<NativeState>('bootstrap')) },
  async enableDemo(): Promise<Bootstrap> { return isDemo ? demoApi.bootstrap() : state(await call<NativeState>('enable_demo')) },
  async switchAccount(id: string): Promise<Bootstrap> { return isDemo ? demoApi.switchAccount(id) : state(await call<NativeState>('switch_account', { id })) },
  async refresh(id: string, range: UsageRange, force = false): Promise<Bootstrap> { return isDemo ? demoApi.refresh(id, range) : state(await call<NativeState>('refresh', { id, range, force })) },
  async login(input: LoginInput): Promise<LoginResult> {
    if (isDemo) throw new Error('示例预览不接收真实账号，请在桌面应用中登录。')
    const r = await call<{ status: 'success' | 'twoFactor'; challengeId?: string; state?: NativeState }>('login', {
      input: {
        site: input.siteUrl,
        email: input.email,
        password: input.password,
        alias: input.alias,
        remember: input.remember,
      },
    })
    return { ...r, state: r.state ? state(r.state) : undefined }
  },
  async twoFactor(challengeId: string, code: string): Promise<LoginResult> { const r = await call<{ status: 'success' | 'twoFactor'; state?: NativeState }>('complete_2fa', { challengeId, code }); return { ...r, state: r.state ? state(r.state) : undefined } },
  async savePreferences(id: string, preferences: AccountPreferences, alias: string): Promise<Bootstrap> { return isDemo ? demoApi.savePreferences(id, preferences, alias) : state(await call<NativeState>('save_preferences', { id, preferences: { ...preferences, alias } })) },
  async patchSettings(patch: AppSettingsPatch, expected?: AppSettingsPatch): Promise<Bootstrap> { return isDemo ? demoApi.patchSettings(patch, expected) : state(await call<NativeState>('patch_settings', { patch, expected })) },
  async removeAccount(id: string): Promise<Bootstrap> { return isDemo ? demoApi.removeAccount(id) : state(await call<NativeState>('remove_account', { id })) },
  async logout(id: string): Promise<Bootstrap> { return isDemo ? demoApi.logout(id) : state(await call<NativeState>('logout', { id })) },
  async records(query: QueryInput): Promise<UsagePage> {
    if (isDemo) return demoApi.records(query)
    const r = await call<{
      accountId: string; generation: number; range: UsageRange; start: string; end: string;
      timezone: string; items: NativeRecord[]; total: number; page: number; pageSize: number;
      complete: boolean; message?: string | null;
    }>('query_records', {
      query: {
        accountId: query.accountId,
        range: query.range,
        page: query.page,
        pageSize: query.pageSize,
        apiKeyId: query.keyId ? Number(query.keyId) : null,
        model: query.model || null,
      },
    })
    return { ...r, items: r.items.map(record) }
  },
  async analysis(accountId: string, range: UsageRange, includeKeys = false, force = false): Promise<UsageAnalysis> {
    if (isDemo) return demoApi.analysis(accountId, range)
    type Row = { name: string; totals: NativeTotals }
    const r = await call<{
      accountId: string; generation: number; range: UsageRange; start: string; end: string;
      timezone: string; syncedAt: string; trend: Row[]; models: Row[]; keys: Row[];
      complete: boolean; message?: string;
    }>('analysis', { query: { accountId, range, apiKeyId: null, model: null, includeKeys, force } })
    return {
      ...r,
      trend: r.trend.map(row => ({ label: row.name, actualCost: Number(row.totals.cost) })),
      models: r.models.map(row => ({ name: row.name, ...totals(row.totals) })),
      keys: r.keys.map(row => ({ name: row.name, ...totals(row.totals) })),
    }
  },
  async subscribe(callback: (data: Bootstrap) => void): Promise<() => void> { return isNative ? listen<NativeState>('subgauge:state', e => callback(state(e.payload))) : () => {} },
  async subscribeSettingsError(callback: (error: string) => void): Promise<() => void> { return isNative ? listen<string>('subgauge:settings-error', e => callback(e.payload)) : () => {} },
  async navigate(callback: (page: string, accountId?: string) => void): Promise<() => void> { return isNative ? listen<{ page: string; accountId?: string }>('subgauge:navigate', e => callback(e.payload.page, e.payload.accountId)) : () => {} },
  async openDetails(target = 'overview') { const separator = target.indexOf(':'); const page = separator < 0 ? target : target.slice(0, separator); const accountId = separator < 0 ? null : target.slice(separator + 1); if (isNative) await call('open_details', { page, accountId }) },
  async showFloat() { if (isNative) await call('show_float') },
  async resize(height: number, expanded: boolean, menuOpen = false, minHeight?: number): Promise<FloatSizeState | null> { return isNative ? call<FloatSizeState>('set_float_layout', { height, minHeight, expanded, menuOpen }) : null },
  async subscribeSize(callback: (size: FloatSizeState) => void): Promise<() => void> { return isNative ? listen<FloatSizeState>('subgauge:float-size', e => callback(e.payload)) : () => {} },
  async startResize(direction: ResizeDirection) { if (isNative) await call('start_float_resize', { direction }) },
  async startDrag() { if (isNative) await call('start_float_drag') },
  async resetSize() { if (isNative) await call('reset_float_size') },
}
