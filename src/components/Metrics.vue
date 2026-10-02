<script setup lang="ts">
import type { Metric, UsageTotals } from '../types'
import { labels, money, compact, tokens, cache } from '../format'
defineProps<{ fields: Metric[]; totals: UsageTotals | null; balance: number | null }>()
function value(field: Metric, totals: UsageTotals | null, balance: number | null) {
  if (field === 'balance') return money(balance)
  if (!totals) return '—'
  return field === 'cost' ? money(totals.actualCost) : field === 'requests' ? compact(totals.requests) : field === 'tokens' ? compact(tokens(totals)) : cache(totals)
}
</script>
<template><div class="metrics"><div v-for="field in fields" :key="field" class="metric"><span class="metric-label">{{ labels[field] }}</span><strong :class="['metric-value', { 'long-number': value(field, totals, balance).length > 10 }]" :title="value(field, totals, balance)">{{ value(field, totals, balance) }}<small v-if="field === 'requests' && totals">次</small></strong></div></div></template>
