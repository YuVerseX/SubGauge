import type { Metric, UsageTotals } from './types'
export const labels: Record<Metric, string> = { cost: '实际扣费', requests: '请求', tokens: 'Token', cache: '缓存率', balance: '当前余额' }
export const ranges = { today: '今日', week: '最近 7 天', month: '本月', recent: '最近几分钟' } as const
export function money(value: number | null | undefined, digits?: number): string {
  if (value == null || !Number.isFinite(value)) return '—'
  const precision = digits ?? (value !== 0 && Math.abs(value) < .01 ? 4 : 2)
  const unit = 10 ** -precision
  if (value !== 0 && Math.abs(value) < unit / 2) return (value < 0 ? '>-$' : '<$') + unit.toFixed(precision)
  return '$' + value.toLocaleString('en-US', { minimumFractionDigits: precision, maximumFractionDigits: precision })
}
export function relativeTime(value: string | null | undefined, now = Date.now()): string {
  if (!value) return '—'
  const time = Date.parse(value)
  if (!Number.isFinite(time) || !Number.isFinite(now)) return '时间不可用'
  const seconds = Math.floor((now - time) / 1000)
  if (seconds < 0) return '时间待核对'
  if (seconds < 60) return '刚刚'
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`
  return `${Math.floor(seconds / 86400)} 天前`
}
export function compact(value: number): string { return value >= 1e6 ? (value / 1e6).toFixed(2) + 'M' : value >= 1000 ? (value / 1000).toFixed(1).replace(/\.0$/, '') + 'K' : value.toLocaleString('en-US') }
export function tokens(value: UsageTotals): number { return value.inputTokens + value.outputTokens + value.cacheReadTokens + value.cacheCreationTokens }
export function cache(value: UsageTotals): string { const input = value.inputTokens + value.cacheReadTokens + value.cacheCreationTokens; return input ? (value.cacheReadTokens / input * 100).toFixed(1) + '%' : '—' }
export function timestamp(value: string | null | undefined, timezone = 'Asia/Shanghai', full = false): string { if (!value) return '尚未同步'; try { return new Intl.DateTimeFormat('zh-CN', { timeZone: timezone, ...(full ? { month: '2-digit', day: '2-digit' } as const : {}), hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }).format(new Date(value)) } catch { return '时间不可用' } }
export function host(site: string): string { try { return new URL(site).host } catch { return site } }
