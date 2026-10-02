<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown, ChevronUp } from '@lucide/vue'
import type { UsageRecord } from '../types'
import { money, compact, tokens, cache, timestamp } from '../format'
defineProps<{ items: UsageRecord[]; timezone: string; loading?: boolean }>()
const selected = ref<string | null>(null)
</script>
<template>
  <div class="table-scroll"><table class="usage-table"><thead><tr><th>时间</th><th>模型 / Key</th><th>Token</th><th>实际扣费</th><th><span class="sr-only">详情</span></th></tr></thead><tbody>
    <template v-for="item in items" :key="item.id"><tr :class="{ selected: selected === item.id }"><td>{{ timestamp(item.createdAt, timezone, true) }}</td><td><span class="model-name">{{ item.model }}</span><small>{{ item.keyName || 'Key #' + item.keyId }}</small></td><td class="numeric">{{ compact(tokens(item)) }}</td><td class="numeric">{{ money(item.actualCost, 4) }}</td><td><button class="icon-button" :aria-label="(selected === item.id ? '收起' : '展开') + '请求 ' + item.id" :aria-expanded="selected === item.id" @click="selected = selected === item.id ? null : item.id"><ChevronUp v-if="selected === item.id" :size="15"/><ChevronDown v-else :size="15"/></button></td></tr>
    <tr v-if="selected === item.id" class="request-detail"><td colspan="5"><div class="token-detail"><span>普通输入<strong>{{ item.inputTokens.toLocaleString() }}</strong></span><span>输出<strong>{{ item.outputTokens.toLocaleString() }}</strong></span><span>缓存读取<strong>{{ item.cacheReadTokens.toLocaleString() }}</strong></span><span>缓存写入<strong>{{ item.cacheCreationTokens.toLocaleString() }}</strong></span><span>缓存率<strong>{{ cache(item) }}</strong></span><span>耗时<strong>{{ item.durationMs == null ? '—' : (item.durationMs / 1000).toFixed(2) + ' 秒' }}</strong></span></div><div class="muted">请求 #{{ item.id }} · {{ timestamp(item.createdAt, timezone, true) }}</div></td></tr></template>
    <tr v-if="!items.length"><td colspan="5" class="empty-state">{{ loading ? '正在读取请求记录…' : '这个范围内没有请求记录' }}</td></tr>
  </tbody></table></div>
</template>
