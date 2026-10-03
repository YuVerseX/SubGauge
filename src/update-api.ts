import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isDemo, isNative } from './api'
import { updateDemo } from './update-demo'
import type { UpdatePreferencesPatch, UpdateState } from './update-types'

type UpdateCommand = 'update_status' | 'check_update' | 'download_update' | 'install_update' | 'save_update_preferences' | 'open_update_release'
async function call<T>(command: UpdateCommand, args?: Record<string, unknown>): Promise<T> {
  if (!isNative) throw new Error('请在 SubGauge 桌面应用中检查更新。')
  return invoke<T>(command, args)
}

export const updateApi = {
  async status(): Promise<UpdateState> { return isDemo ? updateDemo.status() : call('update_status') },
  async check(): Promise<UpdateState> { return isDemo ? updateDemo.check() : call('check_update', { manual: true }) },
  async download(revision: number): Promise<UpdateState> {
    if (isDemo) throw new Error('示例预览不会下载软件更新。')
    return call('download_update', { revision })
  },
  async install(revision: number): Promise<void> {
    if (isDemo) throw new Error('示例预览不会安装软件更新。')
    return call('install_update', { revision })
  },
  async savePreferences(patch: UpdatePreferencesPatch): Promise<UpdateState> {
    return isDemo ? updateDemo.savePreferences(patch) : call('save_update_preferences', patch as Record<string, unknown>)
  },
  async openRelease(revision: number): Promise<void> {
    if (isDemo) throw new Error('示例预览不会打开软件更新链接。')
    return call('open_update_release', { revision })
  },
  async subscribe(callback: (state: UpdateState) => void): Promise<() => void> {
    if (isDemo) return updateDemo.subscribe(callback)
    if (!isNative) return () => {}
    return listen<UpdateState>('subgauge:update', event => callback(event.payload))
  },
}
