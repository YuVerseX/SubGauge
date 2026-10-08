import type { TrendMeta, TrendPoint } from './types'

export interface TrendBucket { key: string; label: string; fullLabel: string; actualCost: number | null }

// A server label without an offset is only a calendar coordinate. It is never
// interpreted as the computer's timezone or converted into the account timezone.
function coordinate(point: TrendPoint, daily: boolean): number | null {
  if (point.bucketStart) {
    const value = Date.parse(point.bucketStart)
    if (!Number.isFinite(value)) return null
    if (!daily) return value
  }
  const match = /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::\d{2})?)?$/.exec(point.label)
  if (!match) return null
  const [, y, m, d, h = '0', min = '0'] = match
  const value = Date.UTC(Number(y), Number(m) - 1, Number(d), Number(h), Number(min))
  const date = new Date(value)
  return date.getUTCFullYear() === Number(y) && date.getUTCMonth() === Number(m) - 1 && date.getUTCDate() === Number(d) && date.getUTCHours() === Number(h) && date.getUTCMinutes() === Number(min) ? value : null
}
function labels(value: number, meta: TrendMeta, timestamp: boolean) {
  const date = new Date(value)
  const formatter = new Intl.DateTimeFormat('sv-SE', { timeZone: timestamp && meta.timezone ? meta.timezone : 'UTC', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
  const full = formatter.format(date)
  return { full, short: meta.granularity === 'day' ? full.slice(5, 10) : full.slice(11, 16) }
}

export function trendBuckets(points: TrendPoint[], meta?: TrendMeta): TrendBucket[] {
  const original = () => points.map((p, i) => ({ key: `${i}-${p.label}`, label: p.label, fullLabel: p.label, actualCost: Number.isFinite(p.actualCost) ? p.actualCost : null }))
  if (!meta || !points.length) return original()
  if (meta.timezone) {
    try { new Intl.DateTimeFormat('en', { timeZone: meta.timezone }).format(0) }
    catch { return original() }
  }
  // Daily points use supplied local dates: adjacent days can be 23 or 25 hours apart.
  const daily = meta.granularity === 'day'
  const located = points.map(point => ({ point, at: coordinate(point, daily) }))
  if (located.some(item => item.at == null)) return original()
  const timestamps = points.every(point => !!point.bucketStart)
  // Mixed timestamp/calendar coordinates and unknown local offsets must not be combined.
  if (points.some(point => !!point.bucketStart) !== timestamps || (timestamps && !meta.timezone)) return original()
  const sorted = located.sort((a, b) => a.at! - b.at!)
  const step = meta.granularity === 'day' ? 86400000 : meta.granularity === 'hour' ? 3600000 : 60000
  const first = sorted[0]!.at!, last = sorted.at(-1)!.at!
  if (new Set(sorted.map(item => item.at)).size !== sorted.length || sorted.some(item => (item.at! - first) % step !== 0)) return original()
  const count = Math.round((last - first) / step) + 1
  if (count > 500 || count < 1) return original()
  const byTime = new Map(sorted.map(item => [item.at!, item.point]))
  return Array.from({ length: count }, (_, index) => {
    const at = first + index * step, point = byTime.get(at)
    const text = labels(at, meta, timestamps && !daily)
    return { key: String(at), label: text.short, fullLabel: point?.label || text.full, actualCost: point ? Number.isFinite(point.actualCost) ? point.actualCost : null : meta.complete && meta.missingBuckets === 'zero' ? 0 : null }
  })
}
