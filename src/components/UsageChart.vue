<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { money } from '../format'
import { trendBuckets } from '../trend'
import type { TrendMeta, TrendPoint } from '../types'
const props = defineProps<{ points: TrendPoint[]; meta?: TrendMeta; loading?: boolean }>()
const buckets = computed(() => trendBuckets(props.points, props.meta))
const peak = computed(() => Math.max(0, ...buckets.value.map(p => p.actualCost ?? 0)))
const scale = computed(() => peak.value || 1)
const active = ref<string | null>(null)
const selected = computed(() => buckets.value.find(p => p.key === active.value))
const stride = computed(() => Math.max(1, Math.ceil(buckets.value.length / 6)))
const hasUnknown = computed(() => buckets.value.some(point => point.actualCost == null))
watch(() => props.points, () => { active.value = null })
</script>
<template>
  <div v-if="buckets.length" class="trend-view">
    <div class="chart" role="group" aria-label="实际扣费趋势">
      <div class="chart-axis"><span>{{ money(peak) }}</span><span>{{ peak ? money(peak / 2) : '' }}</span><span>$0</span></div>
      <div class="chart-plot">
        <button v-for="(point, i) in buckets" :key="point.key" type="button" class="chart-column" :class="{ 'unknown-bucket': point.actualCost == null, 'active-bucket': active === point.key }" :aria-label="point.fullLabel + ' · ' + (point.actualCost == null ? '数据未确认' : money(point.actualCost, 4))" @mouseenter="active = point.key" @mouseleave="active = null" @focus="active = point.key" @blur="active = null">
          <span v-if="point.actualCost == null" class="unknown-mark" aria-hidden="true">·</span>
          <div v-else class="chart-bar" :class="{ 'zero-bar': point.actualCost === 0 }" :style="{ height: Math.max(point.actualCost / scale * 100, point.actualCost > 0 ? 1 : 0) + '%' }"></div>
          <span v-if="i % stride === 0" class="chart-tick">{{ point.label }}</span>
        </button>
      </div>
    </div>
    <p class="chart-reading" aria-live="polite"><template v-if="selected"><span>{{ selected.fullLabel }}</span><strong>{{ selected.actualCost == null ? '该时段数据未确认' : money(selected.actualCost, 4) }}</strong></template><span v-else class="muted">{{ hasUnknown ? '点标记为未确认时段；悬停查看扣费' : '悬停或用键盘查看各时段扣费' }}</span></p>
    <p v-if="meta" class="muted chart-timezone">{{ meta.timezone ? '分桶时区：' + meta.timezone : '站点分桶时间 · 时区未提供' }}</p>
  </div>
  <div v-else class="empty-state chart-empty">{{ loading ? '正在读取趋势…' : '暂无趋势数据' }}</div>
</template>
