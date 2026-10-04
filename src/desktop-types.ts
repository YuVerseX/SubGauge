import type { UpdateDistribution } from './update-types'

export type StartupVisibility = 'float' | 'tray'

export interface DesktopState {
  revision: number
  distribution: UpdateDistribution
  launchAtLogin: boolean
  startupVisibility: StartupVisibility
  shortcut: string | null
  shortcutRegistered: boolean
  startupBlocked: boolean | null
  error: string | null
}

export interface DesktopPreferencesPatch {
  launchAtLogin?: boolean
  startupVisibility?: StartupVisibility
  shortcut?: string | null
}

export interface DesktopPreferencesInput {
  expectedRevision: number
  patch: DesktopPreferencesPatch
}
