<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ArrowUpRight, Download, LoaderCircle, RefreshCw } from '@lucide/vue'
import { isDemo } from '../api'
import { updateApi } from '../update-api'
import type { UpdateState } from '../update-types'

const state = ref<UpdateState | null>(null)
const loading = ref(true), action = ref(''), localError = ref(''), postponed = ref('')
const installConfirming = ref(false)
let disposed = false, unsubscribe: (() => void) | undefined
const distribution = computed(() => ({ installed: '安装版', portable: '免安装版', development: '开发版' }[state.value?.distribution || 'development']))
const working = computed(() => !!action.value || ['checking', 'downloading', 'installing'].includes(state.value?.phase || ''))
const canDownload = computed(() => state.value?.distribution === 'installed' && !!state.value.version && ['available', 'error'].includes(state.value.phase))
const canOpenRelease = computed(() => state.value?.distribution !== 'development' && !!state.value?.version && ['available', 'error', 'ready'].includes(state.value.phase))
const canInstall = computed(() => !isDemo && state.value?.distribution === 'installed' && state.value.phase === 'ready')
const skipped = computed(() => !!state.value?.version && state.value.version === state.value.skippedVersion)
const deferred = computed(() => !!state.value?.version && state.value.version === postponed.value)
const progress = computed(() => {
  const current = state.value
  if (!current?.totalBytes || current.totalBytes <= 0) return undefined
  return Math.min(100, Math.max(0, current.downloadedBytes / current.totalBytes * 100))
})
const phaseLabel = computed(() => ({
  idle: '尚未检查更新', checking: '正在检查更新…', available: `发现新版 ${state.value?.version || ''}`,
  downloading: '正在下载更新…', ready: '下载与签名验证已完成', installing: '正在安装，即将重启…',
  upToDate: isDemo ? '示例状态：当前版本已是最新' : '当前版本已是最新', error: '更新未完成',
}[state.value?.phase || 'idle']))

function accept(value: UpdateState) {
  if (!disposed && (!state.value || value.revision >= state.value.revision)) {
    if (value.phase !== 'ready' || value.version !== state.value?.version) installConfirming.value = false
    state.value = value
  }
}
function message(error: unknown) { return typeof error === 'string' ? error : error instanceof Error ? error.message : '操作未完成，请重试。' }
function bytes(value: number) { return value < 1024 * 1024 ? `${(value / 1024).toFixed(1)} KB` : `${(value / 1024 / 1024).toFixed(1)} MB` }
function date(value: string) { const result = new Date(value); return Number.isNaN(result.getTime()) ? '—' : result.toLocaleString('zh-CN') }
async function perform(name: string, task: () => Promise<UpdateState | void>) {
  if (working.value) return
  action.value = name; localError.value = ''
  try { const result = await task(); if (result) accept(result) }
  catch (error) {
    if (!disposed) localError.value = message(error)
    if (!disposed) try { accept(await updateApi.status()) } catch { /* Preserve the original actionable error. */ }
  } finally { if (!disposed) action.value = '' }
}
async function check() { postponed.value = ''; await perform('check', () => updateApi.check()) }
async function download() { if (canDownload.value && state.value) await perform('download', () => updateApi.download(state.value!.revision)) }
async function install() {
  if (!canInstall.value) return
  installConfirming.value = false
  await perform('install', () => updateApi.install(state.value!.revision))
}
async function openRelease() { if (canOpenRelease.value && state.value) await perform('release', () => updateApi.openRelease(state.value!.revision)) }
async function toggleAutoCheck(event: Event) {
  const input = event.target as HTMLInputElement
  const next = input.checked
  input.checked = state.value?.autoCheck ?? false
  await perform('preferences', () => updateApi.savePreferences({ autoCheck: next }))
}
async function skipVersion() { if (state.value?.version) await perform('preferences', () => updateApi.savePreferences({ skipVersion: state.value!.version })) }
async function restoreVersion() { await perform('preferences', () => updateApi.savePreferences({ skipVersion: null })) }

onMounted(async () => {
  try {
    const stop = await updateApi.subscribe(accept)
    if (disposed) { stop(); return }
    unsubscribe = stop
    accept(await updateApi.status())
  } catch (error) { if (!disposed) localError.value = message(error) }
  finally { if (!disposed) loading.value = false }
})
onUnmounted(() => { disposed = true; unsubscribe?.() })
</script>

<template>
  <section class="update-panel" aria-labelledby="software-update-heading" data-testid="update-panel">
    <div class="section-head"><h3 id="software-update-heading">软件更新</h3><span v-if="state" class="muted">{{ distribution }} · 预览渠道</span></div>
    <p v-if="isDemo" class="muted update-demo">示例预览，不检查、下载或安装真实软件。</p>
    <p v-if="loading" class="loading-line"><LoaderCircle class="spin" :size="14"/>正在读取更新状态…</p>
    <template v-if="state">
      <div class="update-summary"><span class="muted">当前版本</span><strong>{{ state.currentVersion }}</strong></div>
      <div class="update-status" role="status" aria-live="polite"><LoaderCircle v-if="['checking', 'downloading', 'installing'].includes(state.phase)" class="spin" :size="14"/><span>{{ phaseLabel }}</span></div>
      <p v-if="state.checkedAt" class="muted update-last-check">上次检查 {{ date(state.checkedAt) }}</p>
      <p v-if="skipped" class="muted">已跳过此版本的自动提醒，仍可手动更新。<button type="button" class="text-button" :disabled="working" @click="restoreVersion">恢复提醒</button></p>
      <template v-if="state.version && !deferred">
        <p v-if="state.publishedAt" class="muted">发布时间 {{ date(state.publishedAt) }}</p>
        <p v-if="state.notes" class="update-notes">{{ state.notes }}</p>
      </template>
      <div v-if="state.phase === 'downloading'" class="update-progress">
        <progress :value="progress" max="100" aria-label="更新下载进度"/>
        <span class="muted">已下载 {{ bytes(state.downloadedBytes) }}<template v-if="state.totalBytes && state.totalBytes > 0"> / {{ bytes(state.totalBytes) }} · {{ Math.floor(progress || 0) }}%</template><template v-else> · 总大小未知</template></span>
      </div>
      <p v-if="state.distribution === 'portable'" class="muted">免安装版通过发布页下载 ZIP，解压替换前请退出 SubGauge。</p>
      <p v-else-if="state.distribution === 'development' && !isDemo" class="muted">开发版可检查新版；安装更新请使用正式安装版。</p>
      <p v-if="canInstall" class="muted">点击“安装并重启”会退出 SubGauge 并启动安装器。安装器使用当前安装目录；本机账号与设置保存在独立数据目录。</p>
      <label class="check-field update-auto-check"><input type="checkbox" :checked="state.autoCheck" :disabled="working || isDemo" @change="toggleAutoCheck">自动检查更新</label>
      <p class="muted">后台仅检查新版，下载和安装都由你确认。</p>
      <div class="update-actions">
        <button type="button" class="button" :disabled="working" @click="check"><RefreshCw :size="14"/>{{ state.phase === 'checking' ? '检查中…' : '检查更新' }}</button>
        <button v-if="canDownload && !deferred" type="button" class="button primary" :disabled="working" @click="download"><Download :size="14"/>{{ state.phase === 'error' ? '重新下载' : '下载更新' }}</button>
        <button v-if="canOpenRelease && !deferred" type="button" :class="['button', { primary: state.distribution === 'portable' }]" :disabled="working" @click="openRelease">{{ state.distribution === 'portable' ? '下载 ZIP' : '打开发布页' }}<ArrowUpRight :size="14"/></button>
        <button v-if="canInstall && !installConfirming" type="button" class="button primary" :disabled="working" @click="installConfirming = true">{{ state.error ? '重试安装并重启' : '安装并重启' }}</button>
        <button v-if="['available', 'error'].includes(state.phase) && state.version && deferred" type="button" class="button" :disabled="working" @click="postponed = ''">查看更新</button>
        <button v-if="state.phase === 'available' && !deferred" type="button" class="button quiet" :disabled="working" @click="postponed = state.version || ''">稍后</button>
        <button v-if="state.phase === 'available' && !skipped" type="button" class="button quiet" :disabled="working" @click="skipVersion">跳过此版本</button>
      </div>
      <div v-if="canInstall && installConfirming" class="update-confirm" role="group" aria-label="确认安装更新"><p class="muted">现在退出 SubGauge，安装 {{ state.version }} 并重新启动？</p><div class="update-actions"><button type="button" class="button primary" :disabled="working" @click="install">确认安装并重启</button><button type="button" class="button" :disabled="working" @click="installConfirming = false">取消安装</button></div></div>
    </template>
    <div v-if="localError || state?.error" class="alert error-alert update-error" role="alert">{{ localError || state?.error }}</div>
    <button v-if="!state && !loading" type="button" class="button" :disabled="working" @click="perform('status', () => updateApi.status())">重新读取更新状态</button>
  </section>
</template>
