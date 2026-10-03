export type UsageRange = 'today' | 'week' | 'month' | 'recent'
export type Metric = 'cost' | 'requests' | 'tokens' | 'cache' | 'balance'
export interface AccountPreferences {
  defaultRange: UsageRange
  metrics: Metric[]
  recentMinutes: number
  timezone: string
}
export interface AccountSummary {
  id: string
  alias: string
  siteUrl: string
  email: string
  role: string
  preferences: AccountPreferences
  sessionStatus: string
  demo?: boolean
}
export interface UsageTotals {
  actualCost: number
  requests: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheCreationTokens: number
}
export interface UsageRecord extends UsageTotals {
  id: string
  createdAt: string
  model: string
  keyId: string
  keyName: string
  durationMs?: number | null
}
export interface UsageSnapshot {
  accountId: string
  generation: number
  range: UsageRange
  timezone?: string
  start: string
  end: string
  totals: UsageTotals | null
  recent: UsageTotals | null
  latest: UsageRecord | null
  balance: number | null
  balanceUpdatedAt: string | null
  usageUpdatedAt: string | null
  status: string
  message?: string | null
}
export interface AppSettings {
  theme: 'light' | 'dark' | 'system'
  opacity: number
  alwaysOnTop: boolean
  recentRefreshSeconds: number
  summaryRefreshSeconds: number
  backgroundRefreshSeconds: number
}
export type AppSettingsPatch = Partial<AppSettings>
export interface FloatSizeState { width: number; height: number; manualHeight: boolean; resizing: boolean }
export type ResizeDirection = 'North' | 'South' | 'East' | 'West' | 'NorthEast' | 'NorthWest' | 'SouthEast' | 'SouthWest'
export interface Bootstrap {
  generation: number
  accounts: AccountSummary[]
  activeAccountId: string | null
  range: UsageRange
  settings: AppSettings
  snapshot: UsageSnapshot | null
}
export interface UsagePage { accountId: string; generation: number; range?: UsageRange; start?: string; end?: string; timezone?: string; items: UsageRecord[]; total: number; page: number; pageSize: number; complete: boolean; message?: string | null }
export interface UsageBreakdown extends UsageTotals { name: string }
export interface UsageAnalysis {
  accountId: string
  generation: number
  range: UsageRange
  start: string
  end: string
  timezone: string
  syncedAt: string
  trend: { label: string; actualCost: number }[]
  models: UsageBreakdown[]
  keys: UsageBreakdown[]
  complete: boolean
  message?: string
}
export interface LoginInput { siteUrl: string; email: string; password: string; alias: string; remember: boolean }
export interface LoginResult { status: 'success' | 'twoFactor'; challengeId?: string; state?: Bootstrap }
export interface QueryInput { accountId: string; range: UsageRange; page: number; pageSize: number; model?: string; keyId?: string }
