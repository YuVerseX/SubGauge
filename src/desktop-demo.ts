import type { DesktopPreferencesInput, DesktopState } from './desktop-types'

let state: DesktopState = {
  revision: 0, distribution: 'development', launchAtLogin: false, startupVisibility: 'float',
  shortcut: null, shortcutRegistered: false, startupBlocked: null, error: null,
}
const subscribers = new Set<(state: DesktopState) => void>()
function snapshot(): DesktopState { return { ...state } }

export const desktopDemo = {
  async status(): Promise<DesktopState> { return snapshot() },
  async savePreferences(input: DesktopPreferencesInput): Promise<DesktopState> {
    if (input.expectedRevision !== state.revision) throw new Error('启动设置已在其他窗口修改，请重新载入后保存。')
    if (input.patch.launchAtLogin !== undefined || input.patch.shortcut !== undefined) throw new Error('示例预览不会登记开机自启或占用系统快捷键。')
    state = { ...state, ...input.patch, revision: state.revision + 1 }
    for (const subscriber of subscribers) subscriber(snapshot())
    return snapshot()
  },
  async subscribe(callback: (state: DesktopState) => void): Promise<() => void> {
    subscribers.add(callback)
    return () => { subscribers.delete(callback) }
  },
}
