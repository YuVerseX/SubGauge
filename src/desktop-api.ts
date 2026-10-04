import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isDemo, isNative } from './api'
import { desktopDemo } from './desktop-demo'
import type { DesktopPreferencesInput, DesktopState } from './desktop-types'

async function call<T>(command: 'desktop_status' | 'save_desktop_preferences' | 'open_startup_settings', args?: Record<string, unknown>): Promise<T> {
  if (!isNative) throw new Error('请在 SubGauge 桌面应用中配置启动与快捷键。')
  return invoke<T>(command, args)
}

export const desktopApi = {
  async status(): Promise<DesktopState> { return isDemo ? desktopDemo.status() : call('desktop_status') },
  async savePreferences(input: DesktopPreferencesInput): Promise<DesktopState> {
    return isDemo ? desktopDemo.savePreferences(input) : call('save_desktop_preferences', { input })
  },
  async openStartupSettings(): Promise<void> {
    if (isDemo) throw new Error('示例预览不会打开 Windows 设置。')
    return call('open_startup_settings')
  },
  async subscribe(callback: (state: DesktopState) => void): Promise<() => void> {
    if (isDemo) return desktopDemo.subscribe(callback)
    if (!isNative) return () => {}
    return listen<DesktopState>('subgauge:desktop-state', event => callback(event.payload))
  },
}
