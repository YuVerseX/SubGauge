<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { ArrowDown, ArrowUp, ArrowUpRight, Check, ChevronDown, ChevronUp, ChevronsLeft, ChevronsRight, CircleHelp, GripHorizontal, LoaderCircle, Minus, Pin, PinOff, Plus, RefreshCw, Settings2, UsersRound, Wallet, X } from '@lucide/vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { api, isDemo, isNative } from './api'
import type { AccountPreferences, AccountSummary, AppSettings, AppSettingsPatch, Bootstrap, FloatSizeState, Metric, ResizeDirection, UsageAnalysis, UsagePage, UsageRange } from './types'
import { cache, compact, host, labels, money, ranges, timestamp, tokens } from './format'
import Metrics from './components/Metrics.vue'
import UsageTable from './components/UsageTable.vue'
import UsageChart from './components/UsageChart.vue'
import UpdatePanel from './components/UpdatePanel.vue'
import DesktopPanel from './components/DesktopPanel.vue'
import { syncPresentation } from './freshness'

type Page = 'overview' | 'records' | 'analysis' | 'accounts' | 'preferences' | 'settings' | 'login'
const params = new URLSearchParams(location.search)
const details = ref(params.get('window') === 'details')
const page = ref<Page>((params.get('page') as Page) || 'overview')
const data = ref<Bootstrap | null>(null)
const loading = ref(true), busy = ref(false), error = ref(''), notice = ref(''), picker = ref(false), expanded = ref(false)
const root = ref<HTMLElement>(), pickerElement = ref<HTMLElement>()
const account = computed(() => data.value?.accounts.find(a => a.id === data.value?.activeAccountId) || null)
const snapshot = computed(() => data.value?.snapshot?.accountId === account.value?.id ? data.value?.snapshot || null : null)
const timezone = computed(() => snapshot.value?.timezone || account.value?.preferences.timezone || 'Asia/Shanghai')
const range = computed(() => data.value?.range || 'today')
const rangeLabel = computed(() => range.value === 'recent' ? `近 ${account.value?.preferences.recentMinutes || 5} 分钟` : ranges[range.value])
const freshnessNow = ref(Date.now())
const syncState = computed(() => syncPresentation(snapshot.value, data.value?.settings || { summaryRefreshSeconds: 30, recentRefreshSeconds: 10 }, details.value ? ['cost', 'requests', 'tokens', 'cache', 'balance'] : account.value?.preferences.metrics || [], freshnessNow.value, timezone.value))
const stateLabel = computed(() => syncState.value.label)
const good = computed(() => syncState.value.good)
const recordData = ref<UsagePage | null>(null), analysis = ref<UsageAnalysis | null>(null), detailLoading = ref(false), detailError = ref('')
const recordPage = ref(1), modelFilter = ref(''), keyFilter = ref(''), appliedModel = ref(''), appliedKey = ref('')
const totalPages = computed(() => Math.max(1, Math.ceil((recordData.value?.total || 0) / 20)))
const prefId = ref(''), prefAlias = ref(''), pref = reactive<AccountPreferences>({ defaultRange: 'today', metrics: ['cost', 'requests', 'tokens', 'cache'], recentMinutes: 5, timezone: 'Asia/Shanghai' })
const fieldOrder = ref<Metric[]>(['cost', 'requests', 'tokens', 'cache', 'balance'])
const settings = reactive<AppSettings>({ theme: 'light', opacity: 1, alwaysOnTop: true, recentRefreshSeconds: 10, summaryRefreshSeconds: 30, backgroundRefreshSeconds: 120 })
const settingsBase = reactive<AppSettings>({ ...settings })
const settingsFields = Object.keys(settings) as (keyof AppSettings)[]
const settingsLabels: Record<keyof AppSettings, string> = { theme: '外观主题', opacity: '背景不透明度', alwaysOnTop: '浮窗始终置顶', recentRefreshSeconds: '最近请求间隔', summaryRefreshSeconds: '余额和用量间隔', backgroundRefreshSeconds: '后台账号间隔' }
const settingsConflicts = ref<(keyof AppSettings)[]>([]), settingsConfirming = ref(false)
const pinBusy = ref(false), pinError = ref(''), dragBusy = ref(false)
const pinned = computed(() => data.value?.settings.alwaysOnTop ?? true)
const pinLabel = computed(() => pinned.value ? '取消置顶' : '置顶浮窗')
const systemTheme = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(systemTheme.matches)
const selectedTheme = computed(() => data.value?.settings.theme || 'light')
const effectiveTheme = computed(() => selectedTheme.value === 'system' ? systemDark.value ? 'dark' : 'light' : selectedTheme.value)
const floatSize = ref<FloatSizeState>({ width: 316, height: 240, manualHeight: false, resizing: false })
const resizeDirections: ResizeDirection[] = ['North', 'South', 'East', 'West', 'NorthEast', 'NorthWest', 'SouthEast', 'SouthWest']
const form = reactive({ siteUrl: '', email: '', password: '', alias: '', remember: true, code: '' })
const challenge = ref(''), confirmRemove = ref<string | null>(null)
let unlisten: (() => void)[] = [], observer: ResizeObserver | undefined, freshnessTimer: ReturnType<typeof setInterval> | undefined, requestId = 0, resizeTimer: ReturnType<typeof setTimeout> | undefined, loadingKey = '', lastLayout = '', layoutInFlight = false, layoutPending = false, disposed = false, layoutTask: Promise<void> | undefined

function setSetting<K extends keyof AppSettings>(target: AppSettings, field: K, value: AppSettings[K]) { target[field] = value }
function editSettings() { if (!data.value) return; Object.assign(settings, data.value.settings); Object.assign(settingsBase, data.value.settings); settingsConflicts.value = []; settingsConfirming.value = false }
function reconcileSettings(actual: AppSettings) {
  if (page.value !== 'settings') { Object.assign(settings, actual); Object.assign(settingsBase, actual); return }
  for (const field of settingsFields) {
    if (settings[field] === settingsBase[field] || settings[field] === actual[field]) {
      setSetting(settings, field, actual[field]); setSetting(settingsBase, field, actual[field])
      settingsConflicts.value = settingsConflicts.value.filter(item => item !== field)
    } else if (actual[field] !== settingsBase[field] && !settingsConflicts.value.includes(field)) settingsConflicts.value.push(field)
  }
}
function accept(value: Bootstrap) { if (!data.value || value.generation >= data.value.generation) { freshnessNow.value = Date.now(); data.value = value; reconcileSettings(value.settings) } }
function message(e: unknown) { return typeof e === 'string' ? e : e instanceof Error ? e.message : '操作未完成，请重试。' }
async function run(action: () => Promise<void>) { error.value = ''; busy.value = true; try { await action() } catch (e) { error.value = message(e) } finally { busy.value = false } }
async function navigateWithNotice(target: Page, text: string) {
  page.value = target
  await nextTick()
  if (page.value === target) notice.value = text
}
async function selectAccount(id: string) { picker.value = false; await run(async () => accept(await api.switchAccount(id))) }
async function enableDemo() { await run(async () => accept(await api.enableDemo())) }
async function changeRange(e: Event) { const value = (e.target as HTMLSelectElement).value as UsageRange; if (account.value) await run(async () => accept(await api.refresh(account.value!.id, value))) }
async function refresh() { if (account.value) await run(async () => { accept(await api.refresh(account.value!.id, range.value, true)); if (details.value) { if (page.value === 'records') recordPage.value = 1; await loadDetail(true) } }) }
async function open(target: Page, editing?: AccountSummary) { picker.value = false; error.value = ''; if (!details.value && isNative) { await run(() => api.openDetails(editing ? `${target}:${editing.id}` : target)); return } details.value = true; page.value = target; if (target === 'preferences') editPreferences(editing || account.value); if (target === 'settings') editSettings(); if (target === 'login') clearLogin(editing); await loadDetail() }
async function returnFloat() { if (isNative) { await api.showFloat(); await getCurrentWindow().close() } else { details.value = false; picker.value = false } }
async function drag(event: PointerEvent) {
  if (!isNative || event.button !== 0 || dragBusy.value || floatSize.value.resizing) return
  event.preventDefault(); dragBusy.value = true; pinError.value = ''
  let released = false
  const stopPendingDrag = (end: PointerEvent) => { if (end.pointerId === event.pointerId) released = true }
  window.addEventListener('pointerup', stopPendingDrag); window.addEventListener('pointercancel', stopPendingDrag)
  try {
    picker.value = false; await nextTick(); clearTimeout(resizeTimer)
    if (layoutTask) await layoutTask
    await applyLayout()
    if (released) return
    await api.startDrag()
  } catch (e) { error.value = message(e) } finally { window.removeEventListener('pointerup', stopPendingDrag); window.removeEventListener('pointercancel', stopPendingDrag); dragBusy.value = false; resize() }
}
async function togglePin() {
  if (pinBusy.value || !data.value) return
  pinBusy.value = true; pinError.value = ''; error.value = ''
  const previous = pinned.value
  try { accept(await api.patchSettings({ alwaysOnTop: !previous }, { alwaysOnTop: previous })) }
  catch (e) {
    pinError.value = message(e)
    try { accept(await api.bootstrap()) } catch { pinError.value += ' 当前状态未能重新读取，请通过托盘重新显示浮窗后检查。' }
  } finally { pinBusy.value = false }
}
async function settingsError(text: string) {
  if (details.value) error.value = text; else { error.value = ''; pinError.value = text }
  try { accept(await api.bootstrap()) } catch { /* Keep the native failure visible if the state refresh also fails. */ }
}
async function hideFloat() { if (isNative) await run(() => getCurrentWindow().hide()) }
async function minimizeDetails() { if (isNative) await run(() => getCurrentWindow().minimize()) }
async function resetSize() { await run(async () => { await api.resetSize(); lastLayout = ''; resize(); notice.value = '浮窗已恢复默认尺寸' }) }
async function startResize(event: PointerEvent, direction: ResizeDirection) {
  if (!isNative || picker.value || event.button !== 0) return
  event.preventDefault(); event.stopPropagation(); clearTimeout(resizeTimer)
  floatSize.value = { ...floatSize.value, resizing: true }
  try { await api.startResize(direction) } catch (e) { floatSize.value = { ...floatSize.value, resizing: false }; error.value = message(e); resize() }
}
function acceptSize(size: FloatSizeState) { if (!size) return; const ended = floatSize.value.resizing && !size.resizing; floatSize.value = size; if (ended) lastLayout = ''; if (!size.resizing) nextTick(resize) }
function systemThemeChanged(event: MediaQueryListEvent) { systemDark.value = event.matches }
function clearLogin(a?: AccountSummary) { form.siteUrl = a?.siteUrl || ''; form.email = a?.email || ''; form.alias = a?.alias || ''; form.password = ''; form.code = ''; challenge.value = '' }
async function reconnect(a: AccountSummary) { await open('login', a) }
function editPreferences(a: AccountSummary | null) { if (!a) return; prefId.value = a.id; prefAlias.value = a.alias; Object.assign(pref, { ...a.preferences, metrics: [...a.preferences.metrics] }); fieldOrder.value = [...a.preferences.metrics, ...(['cost', 'requests', 'tokens', 'cache', 'balance'] as Metric[]).filter(k => !a.preferences.metrics.includes(k))] }
function moveField(index: number, offset: number) { const next = [...fieldOrder.value]; [next[index], next[index + offset]] = [next[index + offset]!, next[index]!]; fieldOrder.value = next }
async function savePreferences() { await run(async () => { if (!prefAlias.value.trim()) throw new Error('请输入账号名称。'); if (!pref.metrics.length) throw new Error('请至少选择一个常驻指标。'); new Intl.DateTimeFormat('zh-CN', { timeZone: pref.timezone }); accept(await api.savePreferences(prefId.value, { ...pref, metrics: fieldOrder.value.filter(f => pref.metrics.includes(f)) }, prefAlias.value.trim())); await navigateWithNotice('accounts', '账号显示设置已保存') }) }
async function saveSettings() {
  await run(async () => {
    notice.value = ''
    if (settingsConflicts.value.length && data.value) {
      const changed = settingsConflicts.value.map(field => settingsLabels[field]).join('、')
      for (const field of settingsConflicts.value) setSetting(settingsBase, field, data.value.settings[field])
      settingsConflicts.value = []; settingsConfirming.value = true
      throw new Error(`${changed}已在其他入口修改。你的修改已保留，请核对后再次点击“确认并保存”。`)
    }
    const fields = settingsFields.filter(field => settings[field] !== settingsBase[field])
    if (!fields.length) { settingsConfirming.value = false; notice.value = '没有需要保存的更改'; return }
    const patch = Object.fromEntries(fields.map(field => [field, settings[field]])) as AppSettingsPatch
    const expected = Object.fromEntries(fields.map(field => [field, settingsBase[field]])) as AppSettingsPatch
    try {
      accept(await api.patchSettings(patch, expected))
      // Keep edits made while saving, but advance their base to the successful commit.
      const actual = data.value?.settings
      for (const field of fields) if (actual && actual[field] === patch[field]) { setSetting(settingsBase, field, actual[field]); settingsConflicts.value = settingsConflicts.value.filter(item => item !== field) }
      settingsConfirming.value = false; notice.value = '应用设置已保存'
    } catch (e) {
      try { accept(await api.bootstrap()) } catch { /* Preserve the draft and the original failure. */ }
      throw e
    }
  })
}
async function login() { await run(async () => { if (isDemo) throw new Error('当前是示例预览，请在 SubGauge 桌面应用中输入真实账号。'); try { const result = challenge.value ? await api.twoFactor(challenge.value, form.code) : await api.login({ ...form }); if (result.status === 'twoFactor') { challenge.value = result.challengeId || ''; return } if (result.state) accept(result.state); challenge.value = ''; await navigateWithNotice('overview', '账号已连接'); await loadDetail() } finally { form.password = ''; form.code = '' } }) }
async function remove(a: AccountSummary) { await run(async () => { accept(await api.removeAccount(a.id)); confirmRemove.value = null; notice.value = '已移除本机保存的账号' }) }
async function logout(a: AccountSummary) { await run(async () => { accept(await api.logout(a.id)); notice.value = '该账号已退出登录' }) }
async function loadDetail(force = false) {
  if (!details.value || !account.value || !['overview', 'records', 'analysis'].includes(page.value)) return
  const key = [account.value.id, range.value, page.value, recordPage.value, appliedKey.value, appliedModel.value, account.value.preferences.timezone, account.value.preferences.recentMinutes].join('|')
  if (detailLoading.value && loadingKey === key && !force) return
  loadingKey = key
  const id = ++requestId, a = account.value.id, r = range.value, tab = page.value
  detailLoading.value = true; detailError.value = ''
  if (tab !== 'analysis') recordData.value = null
  try {
    const jobs: Promise<void>[] = []
    if (tab !== 'analysis') jobs.push(api.records({ accountId: a, range: r, page: tab === 'overview' ? 1 : recordPage.value, pageSize: tab === 'overview' ? 5 : 20, model: tab === 'records' ? appliedModel.value : undefined, keyId: tab === 'records' ? appliedKey.value : undefined }).then(v => { if (id === requestId && account.value?.id === v.accountId) recordData.value = v }))
    if (tab !== 'records') jobs.push(api.analysis(a, r, tab === 'analysis', force).then(v => { if (id === requestId && account.value?.id === v.accountId && v.range === r) analysis.value = v }))
    const results = await Promise.allSettled(jobs); const failure = results.find(r => r.status === 'rejected'); if (failure?.status === 'rejected') throw failure.reason
  } catch (e) { if (id === requestId) detailError.value = message(e) } finally { if (id === requestId) detailLoading.value = false }
}
async function applyFilters() { appliedModel.value = modelFilter.value.trim(); appliedKey.value = keyFilter.value.trim(); recordPage.value = 1; await loadDetail() }
async function paginate(delta: number) { recordPage.value = Math.max(1, Math.min(totalPages.value, recordPage.value + delta)); await loadDetail() }
async function restartRecords() { recordPage.value = 1; await loadDetail() }
function relativeWidth(cost: number, items: { actualCost: number }[]) { const max = Math.max(...items.map(i => i.actualCost), .000001); return cost / max * 100 + '%' }
function navigate(target: string, id?: string) { const separator = target.indexOf(':'); const next = separator < 0 ? target : target.slice(0, separator); const inlineId = separator < 0 ? '' : target.slice(separator + 1); page.value = (['overview','records','analysis','accounts','preferences','settings','login'].includes(next) ? next : 'overview') as Page; const targetAccount = data.value?.accounts.find(a => a.id === (id || inlineId)); if (page.value === 'preferences') editPreferences(targetAccount || account.value); if (page.value === 'settings') editSettings(); if (page.value === 'login') clearLogin(targetAccount); loadDetail() }
function dismiss(e: PointerEvent) { if (picker.value && !pickerElement.value?.contains(e.target as Node)) picker.value = false }
function escape(e: KeyboardEvent) { if (e.key === 'Escape') { picker.value = false; confirmRemove.value = null } }
watch([() => account.value?.id, range, () => account.value?.preferences.timezone, () => account.value?.preferences.recentMinutes], () => { recordPage.value = 1; recordData.value = null; analysis.value = null; appliedModel.value = ''; appliedKey.value = ''; modelFilter.value = ''; keyFilter.value = ''; loadDetail() })
watch(page, (next, previous) => { notice.value = ''; error.value = ''; if (previous === 'login' && next !== 'login') { form.password = ''; form.code = ''; challenge.value = '' } if (next === 'records') recordData.value = null; loadDetail() })
watch([picker, expanded, details], () => { document.documentElement.classList.toggle('native-float', isNative && !details.value); nextTick(resize) })
watch(data, () => nextTick(resize))
watch(effectiveTheme, theme => {
  document.documentElement.dataset.theme = theme
}, { immediate: true })
watch(selectedTheme, theme => {
  if (isNative) getCurrentWindow().setTheme(theme === 'system' ? null : theme).catch(() => { error.value = '窗口标题栏主题未能更新，页面主题已应用。' })
}, { immediate: true })
function resize() {
  clearTimeout(resizeTimer)
  if (disposed || details.value || !root.value || !isNative || floatSize.value.resizing || dragBusy.value) return
  if (layoutInFlight) { layoutPending = true; return }
  resizeTimer = setTimeout(() => { void applyLayout().catch(() => {}) }, 40)
}
async function applyLayout() {
  clearTimeout(resizeTimer)
  if (disposed || details.value || !root.value || !isNative || floatSize.value.resizing) return
  if (layoutInFlight) { layoutPending = true; if (layoutTask) await layoutTask; return }
  {
    if (!root.value || details.value || floatSize.value.resizing) return
    if (layoutInFlight) { layoutPending = true; return }
    const stage = root.value.parentElement!
    const stageStyle = getComputedStyle(stage)
    const padding = parseFloat(stageStyle.paddingTop) + parseFloat(stageStyle.paddingBottom)
    // A hidden natural-height clone isolates measurement from a manually stretched
    // flex card. Its lifetime is one synchronous layout read, so no observer loop.
    const clone = root.value.cloneNode(true) as HTMLElement
    clone.classList.add('layout-measure'); clone.setAttribute('aria-hidden', 'true'); clone.inert = true
    clone.style.width = `${root.value.getBoundingClientRect().width}px`
    stage.appendChild(clone)
    let naturalHeight: number, minimumHeight: number
    try {
      naturalHeight = clone.getBoundingClientRect().height + padding
      const panel = clone.querySelector<HTMLElement>('.float-expanded')
      minimumHeight = panel ? naturalHeight - panel.getBoundingClientRect().height + Math.min(80, panel.getBoundingClientRect().height) : naturalHeight
    } finally { clone.remove() }
    const menu = pickerElement.value?.querySelector<HTMLElement>('.account-menu')
    let menuHeight = 0
    if (picker.value && menu) {
      const menuStyle = getComputedStyle(menu)
      const naturalHeight = menu.scrollHeight + parseFloat(menuStyle.borderTopWidth) + parseFloat(menuStyle.borderBottomWidth)
      const limit = parseFloat(menuStyle.getPropertyValue('--menu-height-limit'))
      menuHeight = menu.getBoundingClientRect().top - stage.getBoundingClientRect().top + Math.min(naturalHeight, limit) + parseFloat(stageStyle.paddingBottom)
    }
    const height = Math.ceil(Math.max(naturalHeight, menuHeight, 150))
    const minHeight = Math.ceil(Math.max(minimumHeight, 150))
    const key = [height, minHeight, root.value.getBoundingClientRect().width, expanded.value, picker.value].join('|')
    if (key === lastLayout) return
    lastLayout = key
    layoutInFlight = true
    layoutTask = api.resize(height, expanded.value, picker.value, minHeight)
      .then(size => { if (size && !disposed && !floatSize.value.resizing) acceptSize(size) })
      .catch(() => { lastLayout = ''; error.value = '浮窗尺寸未能调整，请通过托盘重新显示浮窗。'; throw new Error(error.value) })
      .finally(() => { layoutInFlight = false; layoutTask = undefined; if (layoutPending && !disposed) { layoutPending = false; nextTick(resize) } })
    await layoutTask
  }
}
onMounted(async () => { freshnessTimer = setInterval(() => { freshnessNow.value = Date.now() }, 5000); document.documentElement.classList.toggle('native-float', isNative && !details.value); document.addEventListener('pointerdown', dismiss); document.addEventListener('keydown', escape); window.addEventListener('resize', resize); systemTheme.addEventListener('change', systemThemeChanged); try { unlisten.push(await api.subscribe(accept), await api.navigate(navigate), await api.subscribeSize(acceptSize), await api.subscribeSettingsError(settingsError)); accept(await api.bootstrap()); navigate(params.get('page') || 'overview', params.get('accountId') || undefined) } catch (e) { error.value = message(e) } finally { loading.value = false } await nextTick(); if (root.value) { observer = new ResizeObserver(resize); observer.observe(root.value); const panel = root.value.querySelector('.float-expanded'); if (panel) observer.observe(panel) } resize() })
onUnmounted(() => { disposed = true; unlisten.forEach(f => f()); observer?.disconnect(); clearTimeout(resizeTimer); clearInterval(freshnessTimer); systemTheme.removeEventListener('change', systemThemeChanged); document.documentElement.classList.remove('native-float'); window.removeEventListener('resize', resize); document.removeEventListener('pointerdown', dismiss); document.removeEventListener('keydown', escape) })
</script>

<template>
  <div :class="['app-stage', details ? 'details-stage' : 'float-stage', { 'browser-stage': !isNative, 'manual-height': floatSize.manualHeight && !picker, 'is-expanded': expanded }]" :data-theme="effectiveTheme">
    <template v-if="isNative && !details && !picker"><div v-for="direction in resizeDirections" :key="direction" :class="['resize-edge', 'resize-' + direction]" :data-resize="direction" aria-hidden="true" @pointerdown="startResize($event, direction)"/></template>
    <article ref="root" :class="['app-surface', details ? 'details-window' : 'float-card']" :style="{ '--card-opacity': data?.settings.opacity ?? 1 }">
      <div v-if="isDemo || account?.demo" class="demo-banner">示例预览 · 未连接真实站点</div>
      <header :class="details ? 'chrome' : 'float-header'">
        <button v-if="!details" class="drag-handle" aria-label="拖动浮窗" title="拖动浮窗" :disabled="dragBusy" @pointerdown="drag"><GripHorizontal :size="13"/></button>
        <span v-if="details" class="brand">SubGauge<span class="brand-dot"></span></span>
        <div ref="pickerElement" class="account-switch">
          <button class="account-trigger" :disabled="!account || busy" :aria-expanded="picker" aria-label="切换账号" @click="picker = !picker"><span v-if="account" class="avatar">{{ account.alias.slice(0, 1) }}</span><strong>{{ account?.alias || 'SubGauge' }}</strong><ChevronDown v-if="account" :size="13"/></button>
          <div v-if="picker" class="account-menu"><button v-for="a in data?.accounts" :key="a.id" class="account-option" :aria-pressed="a.id === account?.id" @click="selectAccount(a.id)"><span class="avatar">{{ a.alias.slice(0, 1) }}</span><span><strong>{{ a.alias }}</strong><small>{{ host(a.siteUrl) }} · {{ a.sessionStatus === 'needsLogin' ? '需登录' : a.role === 'admin' ? '管理员' : '普通用户' }}</small></span><Check v-if="a.id === account?.id" :size="15"/></button><button class="menu-manage" @click="open('accounts')"><UsersRound :size="15"/>账号管理</button></div>
        </div>
        <template v-if="!details && account"><select class="period-select" aria-label="统计范围" :value="range" :disabled="busy" @change="changeRange"><option v-for="(label, value) in ranges" :key="value" :value="value">{{ value === 'recent' ? '近 ' + account.preferences.recentMinutes + ' 分钟' : label }}</option></select><button class="icon-button" aria-label="当前账号显示设置" title="当前账号显示设置" @click="open('preferences', account)"><Settings2 :size="15"/></button></template>
        <button v-if="!details" class="icon-button float-hide" aria-label="隐藏到托盘" title="隐藏到托盘" :disabled="!isNative" @click="hideFloat"><Minus :size="14"/></button>
        <div v-if="details" class="chrome-actions"><button class="icon-button" aria-label="账号管理" @click="open('accounts')"><UsersRound :size="17"/></button><button class="icon-button" aria-label="应用设置" @click="open('settings')"><Settings2 :size="17"/></button><button class="icon-button" aria-label="最小化详情窗口" title="最小化详情窗口" :disabled="!isNative" @click="minimizeDetails"><Minus :size="16"/></button><button class="button" @click="returnFloat">返回浮窗</button></div>
      </header>
      <div v-if="loading" class="empty-state"><LoaderCircle class="spin" :size="22"/><p>正在加载 SubGauge…</p></div>
      <template v-else-if="!details">
        <template v-if="account"><Metrics :fields="account.preferences.metrics" :totals="snapshot?.totals || null" :balance="snapshot?.balance ?? null" :balance-exact="snapshot?.balanceExact"/>
          <div v-if="error || pinError || snapshot?.message" class="compact-error" role="status">{{ error || pinError || snapshot?.message }}</div>
          <footer class="float-footer"><button class="sync-status" :title="syncState.title" :disabled="busy" @click="refresh"><span :class="['status-dot', { good }]"/><span>{{ busy ? '同步中' : stateLabel }}</span><RefreshCw :size="11" :class="{ spin: busy }"/></button><div class="float-actions"><button class="icon-button pin-toggle" :aria-label="pinLabel" :title="pinLabel" :aria-pressed="pinned" :aria-busy="pinBusy" :disabled="pinBusy || !data" @click="togglePin"><LoaderCircle v-if="pinBusy" class="spin" :size="14"/><Pin v-else-if="pinned" :size="14"/><PinOff v-else :size="14"/></button><button class="button quiet" :aria-expanded="expanded" @click="expanded = !expanded">{{ expanded ? '收起' : '展开' }}<ChevronUp v-if="expanded" :size="13"/><ChevronDown v-else :size="13"/></button></div></footer>
          <section v-if="expanded" class="float-expanded"><div class="section-head"><h3>最近一笔</h3><span class="muted">{{ snapshot?.latest ? timestamp(snapshot.latest.createdAt, timezone) : '—' }}</span></div><template v-if="snapshot?.latest"><div class="latest-line"><span>{{ snapshot.latest.model }}</span><strong>{{ money(snapshot.latest.actualCost, 4) }}</strong></div><p class="muted">{{ compact(tokens(snapshot.latest)) }} Token · 缓存率 {{ cache(snapshot.latest) }}</p></template><p v-else class="muted">暂无已记录的请求</p><template v-if="range !== 'recent'"><h3>近 {{ account.preferences.recentMinutes }} 分钟</h3><p class="recent-line">{{ money(snapshot?.recent?.actualCost) }}<span>·</span>{{ snapshot?.recent?.requests ?? '—' }} 次<span>·</span>{{ snapshot?.recent ? compact(tokens(snapshot.recent)) : '—' }} Token</p></template><p class="muted scope-small">{{ host(account.siteUrl) }} · 我的全部 Key</p><button v-if="account.sessionStatus === 'needsLogin'" class="button primary wide-button" @click="open('login')">重新登录</button><button v-else class="button wide-button" @click="open('overview')">详细用量<ArrowUpRight :size="15"/></button></section>
        </template>
        <section v-else class="welcome"><div class="welcome-mark">S<span>G</span></div><h2>用量，一眼看清</h2><p>连接你的 Sub2API，查看余额与个人用量。</p><button class="button primary wide-button" @click="open('login')"><Plus :size="16"/>连接账号</button><button v-if="isNative" class="button quiet wide-button" :disabled="busy" @click="enableDemo">体验示例 · 本地模拟数据</button><p v-if="error || pinError" class="form-error" role="alert">{{ error || pinError }}</p></section>
        <footer v-if="!account" class="float-footer welcome-footer"><span class="sync-status">等待连接</span><div class="float-actions"><button class="icon-button pin-toggle" :aria-label="pinLabel" :title="pinLabel" :aria-pressed="pinned" :aria-busy="pinBusy" :disabled="pinBusy || !data" @click="togglePin"><LoaderCircle v-if="pinBusy" class="spin" :size="14"/><Pin v-else-if="pinned" :size="14"/><PinOff v-else :size="14"/></button></div></footer>
      </template>
      <template v-else>
        <nav class="tabs" aria-label="用量页面"><button v-for="(title, id) in { overview: '用量概览', records: '请求记录', analysis: '用量分析' }" :key="id" :class="{ active: page === id }" :aria-current="page === id ? 'page' : undefined" @click="page = id">{{ title }}</button></nav>
        <main class="main-content">
          <div v-if="error" class="alert error-alert" role="alert">{{ error }}<button class="icon-button" aria-label="关闭错误提示" @click="error = ''"><X :size="15"/></button></div>
          <div v-if="notice" class="alert success-alert" role="status"><Check :size="15"/>{{ notice }}</div>
          <template v-if="['overview', 'records', 'analysis'].includes(page)">
            <div v-if="!account" class="empty-state"><h2>连接第一个账号</h2><p>管理员和普通用户都可以通过邮箱登录，查看自己的全部 Key。</p><button class="button primary" @click="open('login')">连接账号</button></div>
            <template v-else><div class="page-heading"><div><h1>{{ page === 'overview' ? '用量概览' : page === 'records' ? '请求记录' : '用量分析' }}</h1><p class="muted">{{ host(account.siteUrl) }} · 我的全部 Key</p></div><div class="heading-actions"><button class="icon-button" aria-label="立即刷新" :disabled="busy" @click="refresh"><RefreshCw :size="17" :class="{ spin: busy }"/></button><select class="period-select" aria-label="统计范围" :value="range" :disabled="busy" @change="changeRange"><option v-for="(label, value) in ranges" :key="value" :value="value">{{ value === 'recent' ? '近 ' + account.preferences.recentMinutes + ' 分钟' : label }}</option></select></div></div>
              <p class="muted range-description">{{ snapshot ? timestamp(snapshot.start, timezone, true) + ' — ' + timestamp(snapshot.end, timezone, true) : '正在获取统计范围' }} · {{ timezone }}<template v-if="range === 'week'"> · 滚动 168 小时</template><template v-if="range === 'month'"> · 自然月累计</template></p>
              <div v-if="!good && snapshot" class="alert" role="status">{{ snapshot.message || stateLabel }}<button v-if="account.sessionStatus === 'needsLogin'" class="button" @click="reconnect(account)">重新登录</button></div>
              <Metrics :fields="['cost','requests','tokens','cache']" :totals="snapshot?.totals || null" :balance="snapshot?.balance ?? null" :balance-exact="snapshot?.balanceExact"/>
              <div class="balance-line"><Wallet :size="16"/><span>当前余额</span><strong>{{ money(snapshot?.balance) }}</strong><span class="muted">{{ timestamp(snapshot?.balanceUpdatedAt, timezone) }} · 与统计范围无关</span></div>
              <div v-if="detailError" class="alert error-alert" role="alert">{{ detailError }}<button class="button" @click="loadDetail()">重试</button></div>
              <div v-if="detailLoading" class="loading-line" role="status"><LoaderCircle :size="14" class="spin"/>正在读取{{ page === 'records' ? '请求记录' : '用量分析' }}…</div>
              <p v-if="page !== 'records' && analysis" class="muted range-description">图表更新 {{ timestamp(analysis.syncedAt, analysis.timezone) }} · {{ timestamp(analysis.start, analysis.timezone, true) }} — {{ timestamp(analysis.end, analysis.timezone, true) }} · {{ analysis.timezone }}</p>
              <p v-if="page === 'records' && recordData && !recordData.complete" class="warning-text" role="status">{{ recordData.message || '请求数据变化，当前列表尚未完整确认。' }} <button class="button quiet" :disabled="detailLoading" @click="restartRecords">刷新第一页</button></p><p v-if="page === 'records' && recordData?.end" class="muted range-description">列表截止 {{ timestamp(recordData.end, recordData.timezone || timezone, true) }}<template v-if="range === 'week' || range === 'recent'"> · 翻页沿用本次时间窗口</template><template v-if="recordData.complete && recordData.message"> · {{ recordData.message }}</template></p>
              <template v-if="page === 'overview'"><div class="recent-strip"><strong>近 {{ account.preferences.recentMinutes }} 分钟</strong><span>{{ money(snapshot?.recent?.actualCost) }} 扣费</span><span>{{ snapshot?.recent?.requests ?? '—' }} 次请求</span><span>{{ snapshot?.recent ? compact(tokens(snapshot.recent)) : '—' }} Token</span></div><div class="overview-columns"><section><div class="section-head"><h3>{{ rangeLabel }}消耗</h3><span class="muted">USD</span></div><UsageChart :points="analysis?.trend || []"/></section><section><h3>Token 构成</h3><dl class="token-breakdown"><div v-for="(title, key) in { inputTokens: '普通输入', cacheReadTokens: '缓存读取', cacheCreationTokens: '缓存写入', outputTokens: '输出' }" :key="key"><dt>{{ title }}</dt><dd>{{ snapshot?.totals ? compact(snapshot.totals[key]) : '—' }}</dd></div></dl><p class="muted help-note"><CircleHelp :size="12"/>缓存率按输入 Token 加权</p></section></div><p v-if="analysis && (analysis.message || !analysis.complete)" :class="analysis.complete ? 'muted' : 'warning-text'">{{ analysis.message || '分析数据尚未完整同步' }}</p><div class="section-head"><h3>最近请求</h3><button class="button quiet" @click="page = 'records'">全部记录<ArrowUpRight :size="14"/></button></div><UsageTable :items="recordData?.items || []" :timezone="recordData?.timezone || timezone" :loading="detailLoading"/></template>
              <template v-if="page === 'records'"><form class="filters" @submit.prevent="applyFilters"><label>模型<input v-model="modelFilter" placeholder="全部模型" autocomplete="off"></label><label>Key ID<input v-model="keyFilter" placeholder="全部 Key" inputmode="numeric" pattern="[0-9]*"></label><button class="button" :disabled="detailLoading">筛选</button><button type="button" class="button quiet" @click="modelFilter = ''; keyFilter = ''; applyFilters()">重置</button></form><p class="muted">筛选只作用于下方记录，卡片继续显示全部 Key 汇总。</p><UsageTable :items="recordData?.items || []" :timezone="recordData?.timezone || timezone" :loading="detailLoading"/><div class="pagination"><span class="muted">共 {{ recordData?.total ?? '—' }} 条{{ recordData && !recordData.complete ? ' · 数据不完整' : '' }}</span><div><button class="icon-button" aria-label="上一页" :disabled="recordPage <= 1 || detailLoading" @click="paginate(-1)"><ChevronsLeft :size="16"/></button><span>{{ recordPage }} / {{ totalPages }}</span><button class="icon-button" aria-label="下一页" :disabled="recordPage >= totalPages || detailLoading" @click="paginate(1)"><ChevronsRight :size="16"/></button></div></div></template>
              <template v-if="page === 'analysis'"><p v-if="analysis && (analysis.message || !analysis.complete)" :class="analysis.complete ? 'muted' : 'warning-text'">{{ analysis.message || '分析数据不完整，以下为已同步部分' }}</p><div class="analysis-columns"><section v-for="group in [{ title: '模型消耗', items: analysis?.models || [] }, { title: 'Key 消耗', items: analysis?.keys || [] }]" :key="group.title"><h3>{{ group.title }}</h3><div v-if="!group.items.length" class="empty-state">{{ detailLoading ? '正在读取…' : '暂无分布数据' }}</div><div v-for="item in group.items" :key="item.name" class="rank-row"><div class="rank-heading"><strong>{{ item.name }}</strong><span>{{ money(item.actualCost, 3) }}</span></div><div class="rank-track"><div :style="{ width: relativeWidth(item.actualCost, group.items) }"/></div><div class="muted rank-caption"><span>{{ item.requests }} 次 · {{ compact(tokens(item)) }} Token</span><span>缓存 {{ cache(item) }}</span></div></div></section></div></template>
            </template>
          </template>
          <template v-else-if="page === 'accounts'"><div class="page-heading"><div><h1>账号管理</h1><p class="muted">每个站点与登录账号独立保存</p></div><button class="button primary" @click="open('login')"><Plus :size="16"/>添加账号</button></div><div v-if="!data?.accounts.length" class="empty-state">还没有连接的账号</div><section v-for="a in data?.accounts" :key="a.id" class="account-row"><span class="avatar large-avatar">{{ a.alias.slice(0, 1) }}</span><div class="account-info"><strong>{{ a.alias }}</strong><span class="role-badge">{{ a.role === 'admin' ? '管理员' : '普通用户' }}{{ a.id === account?.id ? ' · 当前' : '' }}</span><p>{{ host(a.siteUrl) }} · {{ a.email }}</p><p>默认{{ ranges[a.preferences.defaultRange] }} · {{ a.preferences.metrics.map(f => labels[f]).join(' / ') }}</p><span :class="a.sessionStatus === 'needsLogin' ? 'warning-text' : 'muted'">{{ a.sessionStatus === 'needsLogin' ? '会话已过期，请重新登录' : '账号已连接' }}</span></div><div class="account-actions"><button class="button" @click="open('preferences', a)">显示设置</button><button v-if="a.sessionStatus === 'needsLogin'" class="button" @click="reconnect(a)">重新登录</button><button v-else class="button" :disabled="a.id === account?.id || busy" @click="selectAccount(a.id)">切换查看</button><button class="text-button muted" :disabled="busy" @click="logout(a)">退出登录</button><button class="text-button danger" @click="confirmRemove = a.id">移除</button></div><div v-if="confirmRemove === a.id" class="remove-confirm"><span>仅移除本机账号与保存的会话，不删除站点数据。</span><button class="button danger" :disabled="busy" @click="remove(a)">确认移除</button><button class="button" @click="confirmRemove = null">取消</button></div></section></template>
          <form v-else-if="page === 'preferences'" class="form-column" @submit.prevent="savePreferences"><h1>账号显示设置</h1><p class="muted">这些设置只影响 {{ prefAlias }}</p><label class="field">账号名称<input v-model="prefAlias" maxlength="24" required></label><div class="form-grid"><label class="field">默认统计范围<select v-model="pref.defaultRange"><option v-for="(label, value) in ranges" :key="value" :value="value">{{ label }}</option></select></label><label class="field">最近窗口（分钟）<input v-model.number="pref.recentMinutes" type="number" min="1" max="120" required></label></div><label class="field">统计时区<input v-model="pref.timezone" list="timezones" required><datalist id="timezones"><option>Asia/Shanghai</option><option>UTC</option><option>America/New_York</option><option>Europe/London</option><option>Asia/Tokyo</option></datalist></label><h3>常驻指标</h3><p class="muted">至少选择一项，使用箭头调整显示顺序。</p><div class="preference-list"><div v-for="(field, index) in fieldOrder" :key="field" class="preference-row"><label><input v-model="pref.metrics" type="checkbox" :value="field" :disabled="pref.metrics.length === 1 && pref.metrics.includes(field)">{{ labels[field] }}<small v-if="field === 'balance'">始终为当前值</small></label><button type="button" class="icon-button" :aria-label="'上移' + labels[field]" :disabled="index === 0" @click="moveField(index, -1)"><ArrowUp :size="14"/></button><button type="button" class="icon-button" :aria-label="'下移' + labels[field]" :disabled="index === fieldOrder.length - 1" @click="moveField(index, 1)"><ArrowDown :size="14"/></button></div></div><div class="form-actions"><button class="button primary" :disabled="busy">保存设置</button><button type="button" class="button" @click="page = 'accounts'">取消</button></div></form>
          <section v-else-if="page === 'settings'" class="form-column"><form @submit.prevent="saveSettings">
            <h1>应用设置</h1><p class="muted">调整常驻浮窗与同步节奏</p>
            <p v-if="settingsConflicts.length" class="warning-text settings-conflict" role="status">{{ settingsConflicts.map(field => settingsLabels[field]).join('、') }}已在其他入口修改；未保存的修改仍保留，保存前需要重新确认。</p>
            <label class="field">外观主题<select v-model="settings.theme" aria-label="外观主题"><option value="light">浅色</option><option value="dark">深色</option><option value="system">跟随系统</option></select></label>
            <label class="check-field"><input v-model="settings.alwaysOnTop" type="checkbox">浮窗始终置顶</label><p class="muted">只控制浮窗；卡片图钉和托盘中的开关与这里同步。</p>
            <label class="field">背景不透明度 <strong>{{ Math.round(settings.opacity * 100) }}%</strong><input v-model.number="settings.opacity" type="range" min="0.25" max="1" step="0.05"></label><p class="muted">只调整卡片背景，文字和菜单保持清晰。</p>
            <div class="size-reset"><button type="button" class="button" :disabled="busy || !isNative" @click="resetSize">恢复默认尺寸</button><p class="muted">清除收起和展开两种状态的手动尺寸；拖动浮窗边缘可重新调整。</p></div>
            <h3>刷新间隔</h3><div class="form-grid"><label class="field">最近请求（秒）<input v-model.number="settings.recentRefreshSeconds" type="number" min="5" max="300" required></label><label class="field">余额和用量（秒）<input v-model.number="settings.summaryRefreshSeconds" type="number" min="10" max="3600" required></label><label class="field">后台账号（秒）<input v-model.number="settings.backgroundRefreshSeconds" type="number" min="30" max="3600" required></label></div>
            <p class="muted">实时用量以站点已记录的请求为准，不包含生成过程中的逐 Token 进度。</p><div class="form-actions"><button class="button primary" :disabled="busy">{{ settingsConfirming ? '确认并保存' : '保存设置' }}</button><button type="button" class="button" @click="page = 'overview'">返回概览</button></div>
            </form><DesktopPanel/>
            <UpdatePanel/>
          </section>
          <form v-else-if="page === 'login'" class="form-column login-form" @submit.prevent="login"><h1>{{ challenge ? '完成二次验证' : '连接 Sub2API' }}</h1><p class="muted">{{ challenge ? '输入身份验证器中的 6 位验证码。' : '通过邮箱登录，查看这个账号的余额和全部 Key 用量。' }}</p><div v-if="isDemo" class="alert">当前是示例预览，不接收真实凭据。</div><template v-if="!challenge"><label class="field">账号名称<input v-model="form.alias" maxlength="24" placeholder="例如：自建站 / 上游套餐" :disabled="isDemo" autocomplete="off"></label><label class="field">站点地址<input v-model="form.siteUrl" type="url" placeholder="https://sub2api.example.com" :disabled="isDemo" required autocomplete="url"></label><label class="field">邮箱<input v-model="form.email" type="email" placeholder="you@example.com" :disabled="isDemo" required autocomplete="username"></label><label class="field">密码<input v-model="form.password" type="password" :disabled="isDemo" required autocomplete="current-password"></label><label class="check-field"><input v-model="form.remember" type="checkbox" :disabled="isDemo">记住登录</label><p class="muted">密码不会保存。记住登录后，会话仅在当前 Windows 用户下加密保存。</p></template><label v-else class="field">验证码<input v-model="form.code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" autocomplete="one-time-code" autofocus required></label><div class="form-actions"><button class="button primary" :disabled="busy || isDemo"><LoaderCircle v-if="busy" class="spin" :size="15"/>{{ busy ? '正在连接…' : challenge ? '验证并登录' : '登录并连接' }}</button><button type="button" class="button" :disabled="busy" @click="clearLogin(); page = 'accounts'">取消</button></div></form>
        </main>
        <footer class="detail-footer"><span class="sync-status"><span :class="['status-dot', { good }]"/>{{ account?.alias || 'SubGauge' }} · {{ account ? stateLabel : '等待连接' }}<template v-if="snapshot?.usageUpdatedAt"> {{ timestamp(snapshot.usageUpdatedAt, timezone) }}</template></span><span>{{ timezone }} · USD</span></footer>
      </template>
    </article>
  </div>
</template>
