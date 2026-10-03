import type { UpdatePreferencesPatch, UpdateState } from './update-types'

let state: UpdateState = {
  revision: 0, currentVersion: '0.1.8（示例）', distribution: 'development', channel: 'preview',
  phase: 'idle', autoCheck: false, checkedAt: null, version: null, notes: null,
  publishedAt: null, downloadedBytes: 0, totalBytes: null, error: null, skippedVersion: null,
}
const subscribers = new Set<(state: UpdateState) => void>()
function snapshot() { return { ...state } }
function publish(patch: Partial<UpdateState>) {
  state = { ...state, ...patch, revision: state.revision + 1 }
  for (const subscriber of subscribers) subscriber(snapshot())
  return snapshot()
}

export const updateDemo = {
  async status() { return snapshot() },
  async check() {
    return publish({ phase: 'upToDate', checkedAt: new Date().toISOString(), error: null })
  },
  async savePreferences(patch: UpdatePreferencesPatch) {
    return publish({
      ...(patch.autoCheck !== undefined ? { autoCheck: patch.autoCheck } : {}),
      ...(patch.skipVersion !== undefined ? { skippedVersion: patch.skipVersion } : {}),
    })
  },
  async subscribe(callback: (state: UpdateState) => void) {
    subscribers.add(callback)
    return () => { subscribers.delete(callback) }
  },
}
