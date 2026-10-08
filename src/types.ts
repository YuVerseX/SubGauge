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
  actualCostExact?: string
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
  balanceExact?: string | null
  balanceUpdatedAt: string | null
  usageUpdatedAt: string | null
  recentUpdatedAt?: string | null
  status: string
  message?: string | null
  sync?: {
    usage: SyncPart
    balance: SyncPart
    recent: SyncPart
    latest?: SyncPart
  }
}
export interface SyncPart { state: string; message: string | null; syncedAt: string | null; complete: boolean }
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
export interface UsageBreakdown extends UsageTotals { name: string; keyId?: number }
export interface TrendPoint { label: string; actualCost: number; bucketStart?: string }
export interface TrendMeta {
  source: 'server' | 'records'
  granularity: 'hour' | 'day' | 'minute'
  timezone: string | null
  missingBuckets: 'unknown' | 'zero'
  complete: boolean
}
export interface FilterOptions {
  accountId: string
  generation: number
  range: UsageRange
  start: string
  end: string
  timezone: string
  models: string[]
  keys: { id: number; name: string }[]
  modelsComplete: boolean
  keysComplete: boolean
  message?: string | null
  syncedAt: string
}
export interface UsageAnalysis {
  accountId: string
  generation: number
  range: UsageRange
  start: string
  end: string
  timezone: string
  syncedAt: string
  trend: TrendPoint[]
  trendMeta?: TrendMeta
  models: UsageBreakdown[]
  keys: UsageBreakdown[]
  complete: boolean
  message?: string
}
export interface LoginInput { siteUrl: string; email: string; password: string; alias: string; remember: boolean }
export interface LoginResult { status: 'success' | 'twoFactor'; challengeId?: string; state?: Bootstrap }
export interface QueryInput { accountId: string; range: UsageRange; page: number; pageSize: number; model?: string; keyId?: string }
