<script setup lang="ts">
import { ref, watch } from 'vue'
import { ChevronDown, ChevronUp } from '@lucide/vue'
import type { UsageRecord } from '../types'
import { money, compact, tokens, cache, timestamp } from '../format'

const props = defineProps<{ items: UsageRecord[]; timezone: string; loading?: boolean; filtered?: boolean }>()
const selected = ref<string | null>(null)

watch(() => props.items, items => {
  if (!items.some(item => item.id === selected.value)) selected.value = null
})

function toggle(id: string) { selected.value = selected.value === id ? null : id }
function toggleRow(event: MouseEvent, id: string) {
  // Selecting a number for copying must not collapse the request details.
  if (window.getSelection()?.toString() || (event.target as Element).closest('button, a, input')) return
  toggle(id)
}
function fullMoney(item: UsageRecord) {
  const exact = item.actualCostExact
  if (exact && /^-?\d+(?:\.\d+)?$/.test(exact)) {
    const [integer, fraction] = exact.split('.')
    return '$' + integer!.replace(/\B(?=(\d{3})+(?!\d))/g, ',') + (fraction === undefined ? '' : '.' + fraction)
  }
  return Number.isFinite(item.actualCost) ? '$' + item.actualCost.toLocaleString('en-US', { maximumFractionDigits: 20 }) : '—'
}
function fullTime(value: string) {
  try {
    return new Intl.DateTimeFormat('zh-CN', {
      timeZone: props.timezone, year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false,
    }).format(new Date(value)) + ' · ' + props.timezone
  } catch { return '时间不可用' }
}
</script>

<template>
  <div class="table-scroll">
    <table class="usage-table">
      <thead><tr><th>时间</th><th>模型 / Key</th><th>Token</th><th>实际扣费</th><th><span class="sr-only">详情</span></th></tr></thead>
      <tbody>
        <template v-for="item in items" :key="item.id">
          <tr class="request-row" :class="{ selected: selected === item.id }" @click="toggleRow($event, item.id)">
            <td :title="fullTime(item.createdAt)">{{ timestamp(item.createdAt, timezone, true) }}</td>
            <td>
              <span class="model-name" :title="item.model">{{ item.model }}</span>
              <small class="key-name" :title="(item.keyName || '未命名 Key') + ' · ID ' + item.keyId">{{ item.keyName || 'Key #' + item.keyId }}</small>
            </td>
            <td class="numeric" :title="'Token 合计：' + tokens(item).toLocaleString('en-US')">{{ compact(tokens(item)) }}</td>
            <td class="numeric" :title="'实际扣费：' + fullMoney(item)">{{ money(item.actualCost, 4) }}</td>
            <td>
              <button type="button" class="icon-button" :aria-label="(selected === item.id ? '收起' : '展开') + '请求 ' + item.id" :aria-expanded="selected === item.id" @click.stop="toggle(item.id)">
                <ChevronUp v-if="selected === item.id" :size="15"/><ChevronDown v-else :size="15"/>
              </button>
            </td>
          </tr>
          <tr v-if="selected === item.id" class="request-detail">
            <td colspan="5">
              <div class="request-summary">
                <span>实际扣费<strong>{{ fullMoney(item) }}</strong></span>
                <span>Token 合计<strong>{{ tokens(item).toLocaleString('en-US') }}</strong></span>
              </div>
              <div class="token-detail">
                <span>普通输入<strong>{{ item.inputTokens.toLocaleString('en-US') }}</strong></span>
                <span>输出<strong>{{ item.outputTokens.toLocaleString('en-US') }}</strong></span>
                <span>缓存读取<strong>{{ item.cacheReadTokens.toLocaleString('en-US') }}</strong></span>
                <span>缓存写入<strong>{{ item.cacheCreationTokens.toLocaleString('en-US') }}</strong></span>
                <span>缓存率<strong>{{ cache(item) }}</strong></span>
                <span>耗时<strong>{{ item.durationMs == null ? '—' : (item.durationMs / 1000).toFixed(2) + ' 秒' }}</strong></span>
              </div>
              <div class="request-metadata muted">请求 #{{ item.id }} · {{ fullTime(item.createdAt) }}</div>
            </td>
          </tr>
        </template>
        <tr v-if="!items.length"><td colspan="5" class="empty-state">{{ loading ? '正在读取请求记录…' : filtered ? '没有匹配的请求记录，试试调整或清除筛选' : '这个范围内没有请求记录' }}</td></tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.request-row{cursor:pointer}
.request-row:hover{background:var(--hover)}
.request-row td{user-select:text}
.key-name{overflow:hidden;text-overflow:ellipsis;max-width:230px}
.request-summary{display:flex;gap:28px;flex-wrap:wrap;margin-bottom:18px;font-size:10px;color:var(--muted);white-space:normal}
.request-summary strong{display:block;margin-top:5px;color:var(--strong);font-family:Bahnschrift,'Segoe UI',sans-serif;font-size:14px;font-weight:500;font-variant-numeric:tabular-nums;overflow-wrap:anywhere}
.request-metadata{white-space:normal;overflow-wrap:anywhere}
.request-detail td{user-select:text}
</style>
