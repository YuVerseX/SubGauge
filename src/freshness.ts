import type { AppSettings, Metric, UsageSnapshot } from './types'

export interface SyncPresentation {
  label: string
  good: boolean
  title: string
}

const statusLabels: Record<string, string> = {
  ready: '已同步', synced: '已同步', loading: '同步中', syncing: '同步中',
  error: '同步失败', stale: '数据未更新', partial: '部分数据未更新', incomplete: '数据不完整',
  needsLogin: '需重新登录', expired: '登录已过期', offline: '离线',
}

function staleAfter(seconds: number, fallback: number): number {
  return Math.max(90, (Number.isFinite(seconds) && seconds > 0 ? seconds : fallback) * 3) * 1000
}

function ageText(age: number): string {
  const seconds = Math.floor(age / 1000)
  if (seconds < 1) return '刚刚'
  if (seconds < 60) return `${seconds} 秒前`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes} 分 ${seconds % 60} 秒前`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时 ${minutes % 60} 分前`
  return `${Math.floor(hours / 24)} 天 ${hours % 24} 小时前`
}

function syncTime(value: string | null | undefined, now: number, threshold: number, timezone: string) {
  if (!value) return { available: false, stale: false, text: '尚未同步' }
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return { available: false, stale: false, text: '时间不可用' }
  let formatted: string
  try {
    formatted = new Intl.DateTimeFormat('zh-CN', {
      timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
    }).format(new Date(time))
  } catch {
    // UTC still gives an unambiguous time when an account timezone is unavailable.
    formatted = new Date(time).toISOString()
  }
  if (!Number.isFinite(now) || time > now) {
    return { available: false, stale: false, text: `${formatted} · 时间异常（晚于本机时间）` }
  }
  const age = now - time
  return { available: true, stale: age > threshold, text: `${formatted} · ${ageText(age)}${age > threshold ? '（数据未更新）' : ''}` }
}

/** Only the displayed data determines freshness; native errors always retain their status. */
export function syncPresentation(
  snapshot: UsageSnapshot | null | undefined,
  settings: Pick<AppSettings, 'summaryRefreshSeconds' | 'recentRefreshSeconds'>,
  fields: readonly Metric[],
  now = Date.now(),
  timezone = snapshot?.timezone || 'Asia/Shanghai',
): SyncPresentation {
  const summaryThreshold = staleAfter(settings.summaryRefreshSeconds, 30)
  const recentThreshold = staleAfter(settings.recentRefreshSeconds, 10)
  const usage = syncTime(snapshot?.usageUpdatedAt, now, snapshot?.range === 'recent' ? recentThreshold : summaryThreshold, timezone)
  const balance = syncTime(snapshot?.balanceUpdatedAt, now, summaryThreshold, timezone)
  const recent = syncTime(snapshot?.recentUpdatedAt, now, recentThreshold, timezone)
  const title = [
    `用量：${usage.text}`,
    `余额：${balance.text}`,
    `最近窗口：${recent.text}`,
    ...(snapshot?.message ? [`状态说明：${snapshot.message}`] : []),
  ].join('\n')
  if (!snapshot) return { label: '等待同步', good: false, title }
  if (!['ready', 'synced'].includes(snapshot.status)) {
    return { label: statusLabels[snapshot.status] || '等待同步', good: false, title }
  }
  const usesUsage = fields.some(field => field !== 'balance')
  const required = usesUsage ? [usage] : []
  const missingData = (usesUsage && snapshot.totals == null)
    || (usesUsage && snapshot.range === 'recent' && snapshot.recent == null)
    || (fields.includes('balance') && snapshot.balance == null)
  if (usesUsage && snapshot.range === 'recent') required.push(recent)
  if (fields.includes('balance')) required.push(balance)
  if (required.some(time => time.stale)) return { label: '数据未更新', good: false, title }
  if (missingData || required.some(time => !time.available)) return { label: '尚未同步', good: false, title }
  return { label: '已同步', good: true, title }
}
