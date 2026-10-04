<script setup lang="ts">
import type { Metric, UsageTotals } from '../types'
import { labels, money, compact, tokens, cache } from '../format'
defineProps<{ fields: Metric[]; totals: UsageTotals | null; balance: number | null; balanceExact?: string | null }>()
function value(field: Metric, totals: UsageTotals | null, balance: number | null) {
  if (field === 'balance') return money(balance)
  if (!totals) return '—'
  return field === 'cost' ? money(totals.actualCost) : field === 'requests' ? compact(totals.requests) : field === 'tokens' ? compact(tokens(totals)) : cache(totals)
}
function fullNumber(value: number) { return Number.isFinite(value) ? value.toLocaleString('en-US', { maximumFractionDigits: 20 }) : '—' }
function fullMoney(value: number | null, exact?: string | null) {
  if (exact && /^-?\d+(?:\.\d+)?$/.test(exact)) {
    const [integer, fraction] = exact.split('.')
    return '$' + integer!.replace(/\B(?=(\d{3})+(?!\d))/g, ',') + (fraction === undefined ? '' : '.' + fraction)
  }
  return value == null || !Number.isFinite(value) ? '尚未同步' : '$' + fullNumber(value)
}
function metricTitle(field: Metric, totals: UsageTotals | null, balance: number | null, balanceExact?: string | null) {
  if (field === 'balance') return `当前余额：${fullMoney(balance, balanceExact)}`
  if (!totals) return `${labels[field]}：尚未同步`
  if (field === 'cost') return `实际扣费：${fullMoney(totals.actualCost, totals.actualCostExact)}`
  if (field === 'requests') return `请求：${fullNumber(totals.requests)} 次`
  if (field === 'tokens') return [
    `Token 合计：${fullNumber(tokens(totals))}`,
    `普通输入：${fullNumber(totals.inputTokens)}`,
    `输出：${fullNumber(totals.outputTokens)}`,
    `缓存读取：${fullNumber(totals.cacheReadTokens)}`,
    `缓存写入：${fullNumber(totals.cacheCreationTokens)}`,
  ].join('\n')
  const input = totals.inputTokens + totals.cacheReadTokens + totals.cacheCreationTokens
  return [
    `缓存率：${cache(totals)}${input === 0 ? '（输入 Token 为零）' : ''}`,
    `缓存读取：${fullNumber(totals.cacheReadTokens)}`,
    `输入合计：${fullNumber(input)}（普通输入 + 缓存读取 + 缓存写入）`,
  ].join('\n')
}
</script>
<template><div class="metrics"><div v-for="field in fields" :key="field" class="metric" :title="metricTitle(field, totals, balance, balanceExact)"><span class="metric-label">{{ labels[field] }}</span><strong :class="['metric-value', { 'long-number': value(field, totals, balance).length > 10 }]">{{ value(field, totals, balance) }}<small v-if="field === 'requests' && totals">次</small></strong></div></div></template>
