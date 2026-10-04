<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { LoaderCircle } from '@lucide/vue'
import { isDemo, isNative } from '../api'
import { desktopApi } from '../desktop-api'
import type { DesktopPreferencesPatch, DesktopState, StartupVisibility } from '../desktop-types'

const state = ref<DesktopState | null>(null)
const loading = ref(true), saving = ref(false), localError = ref(''), notice = ref('')
const baseRevision = ref(0)
const draft = reactive({ launchAtLogin: false, startupVisibility: 'float' as StartupVisibility, shortcutEnabled: false, shortcut: 'Ctrl+Alt+G' })
const base = reactive({ launchAtLogin: false, startupVisibility: 'float' as StartupVisibility, shortcut: null as string | null })
let disposed = false, unsubscribe: (() => void) | undefined
const distribution = computed(() => ({ installed: '安装版', portable: '免安装版', development: '开发版' }[state.value?.distribution || 'development']))
const autostartAllowed = computed(() => !isDemo && state.value?.distribution === 'installed')
const canOpenStartupSettings = computed(() => isNative && state.value?.distribution === 'installed')
const shortcutAllowed = computed(() => !isDemo && !!state.value && state.value.distribution !== 'development')
const shortcutValue = computed(() => draft.shortcutEnabled ? draft.shortcut.trim() : null)
const dirty = computed(() => draft.launchAtLogin !== base.launchAtLogin || draft.startupVisibility !== base.startupVisibility || shortcutValue.value !== base.shortcut)
const conflict = computed(() => !!state.value && dirty.value && state.value.revision !== baseRevision.value)
const shortcutStatus = computed(() => !state.value?.shortcut ? '快捷键已关闭' : state.value.shortcutRegistered ? `当前快捷键：${state.value.shortcut}` : `快捷键未生效：${state.value.shortcut}`)

function message(error: unknown) { return typeof error === 'string' ? error : error instanceof Error ? error.message : '启动设置未能保存，请重试。' }
function edit() { notice.value = ''; localError.value = '' }
function resetDraft(value: DesktopState) {
  baseRevision.value = value.revision
  Object.assign(base, { launchAtLogin: value.launchAtLogin, startupVisibility: value.startupVisibility, shortcut: value.shortcut })
  Object.assign(draft, { launchAtLogin: value.launchAtLogin, startupVisibility: value.startupVisibility, shortcutEnabled: value.shortcut !== null, shortcut: value.shortcut || 'Ctrl+Alt+G' })
}
function accept(value: DesktopState) {
  if (disposed || (state.value && value.revision < state.value.revision)) return
  const refreshDraft = !state.value || (!dirty.value && !saving.value)
  state.value = value
  if (refreshDraft) resetDraft(value)
}
function reloadDraft() {
  if (!state.value || saving.value) return
  resetDraft(state.value); localError.value = ''; notice.value = '已载入最新启动设置。'
}
async function readStatus() {
  if (loading.value || saving.value) return
  loading.value = true; localError.value = ''
  try { accept(await desktopApi.status()) }
  catch (error) { if (!disposed) localError.value = message(error) }
  finally { if (!disposed) loading.value = false }
}
async function openStartupSettings() {
  if (!canOpenStartupSettings.value || saving.value) return
  localError.value = ''
  try { await desktopApi.openStartupSettings() }
  catch (error) { if (!disposed) localError.value = message(error) }
}
async function save() {
  if (!state.value || saving.value || !dirty.value || conflict.value) return
  localError.value = ''; notice.value = ''
  if (draft.shortcutEnabled && !shortcutValue.value) { localError.value = '请输入快捷键，或关闭快捷键功能。'; return }
  const patch: DesktopPreferencesPatch = {}
  if (autostartAllowed.value && draft.launchAtLogin !== base.launchAtLogin) patch.launchAtLogin = draft.launchAtLogin
  if (draft.startupVisibility !== base.startupVisibility) patch.startupVisibility = draft.startupVisibility
  if (shortcutAllowed.value && shortcutValue.value !== base.shortcut) patch.shortcut = shortcutValue.value
  if (!Object.keys(patch).length) return
  saving.value = true
  try {
    const result = await desktopApi.savePreferences({ expectedRevision: baseRevision.value, patch })
    if (disposed) return
    accept(result)
    if (state.value) resetDraft(state.value)
    notice.value = result.revision < state.value.revision ? '已载入另一窗口更新后的启动设置。' : '启动设置已保存。'
  } catch (error) {
    if (!disposed) {
      localError.value = message(error)
      try { accept(await desktopApi.status()) } catch { /* Preserve the original save error and the draft. */ }
    }
  } finally { if (!disposed) saving.value = false }
}

onMounted(async () => {
  try {
    const stop = await desktopApi.subscribe(accept)
    if (disposed) { stop(); return }
    unsubscribe = stop
    accept(await desktopApi.status())
  } catch (error) { if (!disposed) localError.value = message(error) }
  finally { if (!disposed) loading.value = false }
})
onUnmounted(() => { disposed = true; unsubscribe?.() })
</script>

<template>
  <section class="desktop-panel" aria-labelledby="desktop-settings-heading" data-testid="desktop-panel">
    <div class="section-head"><h3 id="desktop-settings-heading">启动与后台</h3><span v-if="state" class="muted">{{ distribution }}</span></div>
    <p v-if="isDemo" class="muted">示例预览，不登记开机自启，不占用系统快捷键。</p>
    <p v-if="loading" class="loading-line"><LoaderCircle class="spin" :size="14"/>正在读取启动设置…</p>
    <form v-if="state" @submit.prevent="save">
      <label class="check-field"><input v-model="draft.launchAtLogin" type="checkbox" :disabled="saving || !autostartAllowed" aria-describedby="autostart-description" @change="edit">登录 Windows 后自动启动</label>
      <p id="autostart-description" class="muted">仅为当前 Windows 用户登记，默认关闭。勾选状态表示启动项已登记；是否实际启动，还取决于 Windows 中的启动应用设置。</p>
      <p v-if="!autostartAllowed && !isDemo" class="muted desktop-limited">{{ state.distribution === 'portable' ? '免安装版移动目录后启动路径可能失效。请使用安装版配置开机自启；全局快捷键可在当前运行时使用。' : '开发版不登记开机自启或占用系统快捷键。请使用正式版本配置。' }}</p>
      <p v-if="state.startupBlocked" class="warning-text" role="status">Windows 已禁用 SubGauge 自启。请在 Windows“启动应用”中重新开启；此处保存不会覆盖系统的禁用选择。</p>
      <p v-else-if="state.launchAtLogin && state.startupBlocked === null" class="warning-text" role="status">启动项已登记，但未能确认 Windows 中的禁用状态。Windows 可单独禁用自启，请在“启动应用”中检查；SubGauge 不会自动覆盖系统的选择。</p>
      <button v-if="canOpenStartupSettings" type="button" class="button desktop-windows-settings" :disabled="saving" @click="openStartupSettings">Windows 启动应用设置</button>
      <label class="field"><span>自启时显示</span><select v-model="draft.startupVisibility" :disabled="saving" aria-describedby="startup-visibility-description" @change="edit"><option value="float">显示浮窗</option><option value="tray">仅进入托盘</option></select></label>
      <p id="startup-visibility-description" class="muted">手动打开 SubGauge 会显示浮窗。自启仅进入托盘时，后台同步继续运行。</p>
      <label class="check-field"><input v-model="draft.shortcutEnabled" type="checkbox" :disabled="saving || !shortcutAllowed" aria-describedby="shortcut-description" @change="edit">启用全局显示 / 隐藏快捷键</label>
      <label class="field desktop-shortcut"><span>全局快捷键</span><input v-model="draft.shortcut" type="text" :disabled="saving || !shortcutAllowed || !draft.shortcutEnabled" maxlength="80" placeholder="Ctrl+Alt+G" autocomplete="off" autocapitalize="off" spellcheck="false" aria-describedby="shortcut-description" @input="edit"></label>
      <p id="shortcut-description" class="muted">例如 Ctrl+Alt+G。需包含 Ctrl 或 Alt，可加 Shift，搭配字母、数字或 F1–F11；修改后点击保存。快捷键冲突会显示错误，原设置继续保留。</p>
      <p class="muted desktop-shortcut-status" role="status" aria-live="polite">{{ shortcutStatus }}</p>
      <div v-if="conflict" class="alert desktop-conflict" role="alert"><span>启动设置已在其他窗口修改。你的草稿仍保留，请载入最新设置后重新修改。</span><button type="button" class="button" :disabled="saving" @click="reloadDraft">载入最新启动设置</button></div>
      <div class="desktop-actions"><button type="submit" class="button primary" :disabled="saving || loading || !dirty || conflict"><LoaderCircle v-if="saving" class="spin" :size="14"/>{{ saving ? '保存中…' : '保存启动设置' }}</button><button v-if="dirty && !conflict" type="button" class="button" :disabled="saving" @click="reloadDraft">撤销启动设置更改</button></div>
    </form>
    <p v-if="notice" class="alert success-alert" role="status" aria-live="polite">{{ notice }}</p>
    <p v-if="localError || state?.error" class="alert error-alert" role="alert">{{ localError || state?.error }}</p>
    <button v-if="!state && !loading" type="button" class="button" @click="readStatus">重新读取启动设置</button>
  </section>
</template>

<style scoped>
.desktop-panel{margin-top:32px;padding-top:24px;border-top:1px solid var(--line)}
.section-head{margin-bottom:12px;flex-wrap:wrap}
.desktop-panel>p,.desktop-panel form>p{margin:8px 0;line-height:1.6}
.desktop-shortcut{max-width:290px;margin-bottom:8px}
.desktop-actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:20px}
.desktop-conflict{flex-wrap:wrap;margin-top:14px}
.desktop-conflict>span{flex:1;min-width:190px}
.desktop-conflict .button{white-space:normal}
</style>
