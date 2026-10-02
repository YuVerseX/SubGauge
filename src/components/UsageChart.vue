<script setup lang="ts">
import { computed } from 'vue'
import { money } from '../format'
const props = defineProps<{ points: { label: string; actualCost: number }[] }>()
const max = computed(() => Math.max(...props.points.map(p => p.actualCost), .001))
</script>
<template><div v-if="points.length" class="chart" role="img" aria-label="实际扣费趋势"><div class="chart-axis"><span>{{ money(max) }}</span><span>{{ money(max / 2) }}</span><span>$0</span></div><div class="chart-plot"><div v-for="(point, i) in points" :key="i" class="chart-column" :title="point.label + ' · ' + money(point.actualCost, 4)"><div class="chart-bar" :style="{ height: Math.max(point.actualCost / max * 100, point.actualCost > 0 ? 1 : 0) + '%' }"></div><span v-if="points.length <= 8 || i % Math.ceil(points.length / 7) === 0">{{ point.label }}</span></div></div></div><div v-else class="empty-state chart-empty">暂无趋势数据</div></template>
