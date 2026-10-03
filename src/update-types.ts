export type UpdateDistribution = 'installed' | 'portable' | 'development'
export type UpdatePhase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'installing' | 'upToDate' | 'error'

export interface UpdateState {
  revision: number
  currentVersion: string
  distribution: UpdateDistribution
  channel: 'preview'
  phase: UpdatePhase
  autoCheck: boolean
  checkedAt: string | null
  version: string | null
  notes: string | null
  publishedAt: string | null
  downloadedBytes: number
  totalBytes: number | null
  error: string | null
  skippedVersion: string | null
}

export interface UpdatePreferencesPatch {
  autoCheck?: boolean
  skipVersion?: string | null
}
