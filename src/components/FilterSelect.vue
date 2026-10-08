<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue'
import { Check, ChevronDown, X } from '@lucide/vue'

interface FilterOption { value: string; label: string; description?: string }
const props = withDefaults(defineProps<{
  modelValue: string
  options: FilterOption[]
  label: string
  placeholder?: string
  loading?: boolean
  disabled?: boolean
  allowCustom?: boolean
  customPattern?: string
}>(), { placeholder: '', loading: false, disabled: false, allowCustom: false, customPattern: '' })
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const fieldId = useId()
const listId = fieldId + '-options'
const root = ref<HTMLElement | null>(null)
const input = ref<HTMLInputElement | null>(null)
const open = ref(false)
const query = ref('')
const activeIndex = ref(-1)
const options = computed(() => {
  const seen = new Set<string>()
  return props.options.filter(option => {
    if (!option.value || seen.has(option.value)) return false
    seen.add(option.value)
    return true
  })
})
const selectedLabel = computed(() => options.value.find(option => option.value === props.modelValue)?.label || props.modelValue)
const allLabel = computed(() => '全部' + props.label)
const customValid = computed(() => {
  if (!props.allowCustom || !query.value.trim()) return false
  if (!props.customPattern) return true
  try { return new RegExp('^(?:' + props.customPattern + ')$').test(query.value.trim()) }
  catch { return false }
})
const visibleOptions = computed(() => {
  const text = query.value.trim().toLocaleLowerCase()
  const matches = options.value.filter(option => !text || [option.label, option.value, option.description || ''].some(value => value.toLocaleLowerCase().includes(text)))
  const result: FilterOption[] = text ? [...matches] : [{ value: '', label: allLabel.value }, ...matches]
  if (customValid.value && !options.value.some(option => option.value === query.value.trim())) {
    result.push({ value: query.value.trim(), label: query.value.trim(), description: '使用这个精确值筛选' })
  }
  return result
})
const activeId = computed(() => open.value && activeIndex.value >= 0 && activeIndex.value < visibleOptions.value.length ? listId + '-' + activeIndex.value : undefined)

function resetActive() {
  const desired = query.value.trim() || props.modelValue
  const exact = visibleOptions.value.findIndex(option => option.value === desired)
  activeIndex.value = exact >= 0 ? exact : visibleOptions.value.length ? 0 : -1
}
function revealActive() {
  void nextTick(() => {
    if (activeId.value) document.getElementById(activeId.value)?.scrollIntoView({ block: 'nearest' })
  })
}
function show() {
  if (props.disabled || open.value) return
  query.value = ''
  open.value = true
  resetActive()
}
function close() { open.value = false; query.value = ''; activeIndex.value = -1 }
function choose(value: string) {
  emit('update:modelValue', value)
  input.value?.focus()
  close()
}
function clear() { choose('') }
function onInput(event: Event) {
  query.value = (event.target as HTMLInputElement).value
  open.value = true
  resetActive()
}
function onBlur(event: FocusEvent) {
  if (!root.value?.contains(event.relatedTarget as Node | null)) close()
}
function onKeydown(event: KeyboardEvent) {
  if (event.isComposing) return
  if (event.key === 'Escape') {
    if (open.value) { event.preventDefault(); event.stopPropagation(); close() }
    return
  }
  if (event.key === 'Tab') { close(); return }
  if (event.key === 'Enter') {
    event.preventDefault()
    if (!open.value) show()
    else if (activeIndex.value >= 0) {
      const option = visibleOptions.value[activeIndex.value]
      if (option) choose(option.value)
    }
    return
  }
  if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
  event.preventDefault()
  if (!open.value) show()
  else if (visibleOptions.value.length) {
    const step = event.key === 'ArrowDown' ? 1 : -1
    activeIndex.value = (activeIndex.value + step + visibleOptions.value.length) % visibleOptions.value.length
  }
  revealActive()
}
watch(() => props.modelValue, () => { if (!open.value) query.value = '' })
watch(() => props.disabled, disabled => { if (disabled) close() })
watch(visibleOptions, resetActive)
</script>

<template>
  <div ref="root" class="filter-select" :class="{ 'is-open': open, 'is-disabled': disabled }" @focusout="onBlur">
    <label :for="fieldId">{{ label }}</label>
    <div class="filter-control">
      <input
        :id="fieldId" ref="input" type="text" role="combobox" autocomplete="off" spellcheck="false"
        aria-autocomplete="list" aria-haspopup="listbox" :aria-expanded="open" :aria-controls="listId"
        :aria-activedescendant="activeId" :aria-busy="loading" :disabled="disabled"
        :value="open ? query : selectedLabel" :placeholder="open ? '搜索' + label + '…' : placeholder || allLabel"
        :title="selectedLabel || allLabel" @focus="show" @click="show" @input="onInput" @keydown="onKeydown"
      >
      <button v-if="modelValue" type="button" class="filter-clear" :aria-label="'清除' + label + '筛选'" :disabled="disabled" @mousedown.prevent @click="clear"><X :size="13"/></button>
      <ChevronDown class="filter-chevron" :size="14" aria-hidden="true"/>
    </div>
    <div v-if="open" class="filter-popup">
      <div v-if="loading" class="filter-note" role="status">正在读取选项…</div>
      <div :id="listId" role="listbox" :aria-label="label + '选项'" class="filter-options">
        <div
          v-for="(option, index) in visibleOptions" :id="listId + '-' + index" :key="option.value"
          role="option" :aria-selected="option.value === modelValue" class="filter-option"
          :class="{ 'is-active': activeIndex === index }" :title="option.label + (option.description ? ' · ' + option.description : '')"
          @mousedown.prevent @mouseenter="activeIndex = index" @click="choose(option.value)"
        >
          <span class="filter-option-copy"><strong>{{ option.label }}</strong><small v-if="option.description">{{ option.description }}</small></span>
          <Check v-if="option.value === modelValue" :size="14" aria-hidden="true"/>
        </div>
      </div>
      <div v-if="!visibleOptions.length && !loading" class="filter-note" role="status">没有匹配的{{ label }}{{ allowCustom && customPattern ? '，可输入有效 ID' : '' }}</div>
    </div>
  </div>
</template>

<style scoped>
.filter-select{position:relative;min-width:150px;flex:1;color:var(--ink)}
.filter-select>label{display:block;margin-bottom:6px;color:var(--muted);font-size:11px}
.filter-control{position:relative}
.filter-control input{display:block;width:100%;height:35px;padding:8px 52px 8px 10px;font-size:12px;text-overflow:ellipsis;cursor:text}
.filter-control input::placeholder{color:var(--muted);opacity:1}
.filter-chevron{position:absolute;right:10px;top:11px;color:var(--muted);pointer-events:none}
.filter-clear{position:absolute;right:27px;top:5px;width:25px;height:25px;display:flex;align-items:center;justify-content:center;padding:3px;border-radius:4px;color:var(--muted)}
.filter-clear:hover{background:var(--hover);color:var(--strong)}
.filter-popup{position:absolute;z-index:35;top:calc(100% + 5px);left:0;right:0;min-width:180px;padding:4px;background:var(--surface);border:1px solid var(--border);border-radius:7px;box-shadow:0 6px 20px #0002}
.filter-options{max-height:236px;overflow-y:auto;overscroll-behavior:contain}
.filter-option{display:flex;align-items:center;gap:8px;min-height:34px;padding:8px;border-radius:4px;cursor:pointer;color:var(--secondary)}
.filter-option.is-active{background:var(--hover);color:var(--strong)}
.filter-option[aria-selected=true]{color:var(--strong)}
.filter-option-copy{flex:1;min-width:0}
.filter-option strong{display:block;font-size:12px;font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.filter-option small{display:block;margin-top:3px;color:var(--muted);font-size:10px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.filter-option>svg{flex-shrink:0;color:var(--accent)}
.filter-note{padding:9px 8px;color:var(--muted);font-size:11px;line-height:1.5}
.filter-select.is-disabled{opacity:.55}
@media(max-width:650px){.filter-select{min-width:140px}}
</style>
