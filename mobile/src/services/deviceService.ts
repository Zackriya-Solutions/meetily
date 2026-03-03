/**
 * Device identification for mobile — generates and persists a unique device ID.
 */

import { Preferences } from '@capacitor/preferences'
import { v4 as uuidv4 } from 'uuid'

let _cachedDeviceId: string | null = null

export async function getDeviceId(): Promise<string> {
  if (_cachedDeviceId) return _cachedDeviceId

  try {
    const { value } = await Preferences.get({ key: 'device_id' })
    if (value) {
      _cachedDeviceId = value
      return value
    }
  } catch {
    // Fallback to localStorage in web/dev mode
    const stored = localStorage.getItem('device_id')
    if (stored) {
      _cachedDeviceId = stored
      return stored
    }
  }

  // Generate new device ID
  const newId = uuidv4()
  _cachedDeviceId = newId

  try {
    await Preferences.set({ key: 'device_id', value: newId })
  } catch {
    localStorage.setItem('device_id', newId)
  }

  return newId
}

export function getPlatform(): string {
  const ua = navigator.userAgent.toLowerCase()
  if (ua.includes('iphone') || ua.includes('ipad')) return 'ios'
  if (ua.includes('android')) return 'android'
  return 'mobile'
}
