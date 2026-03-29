/**
 * Secure storage abstraction layer.
 *
 * On native (Capacitor): uses Preferences plugin (always available).
 * Secure-storage community plugin attempted first if available.
 * On web/browser: uses localStorage directly.
 */

import { Preferences } from '@capacitor/preferences'

const isNative = typeof window !== 'undefined' && !!(window as any).Capacitor?.isNativePlatform?.()

let secureStorageModule: any = null
let secureStorageAvailable: boolean | null = null

async function getSecureStorage(): Promise<any | null> {
  if (!isNative || secureStorageAvailable === false) return null
  if (secureStorageModule) return secureStorageModule

  try {
    const mod = await import('@capacitor-community/secure-storage')
    secureStorageModule = mod.SecureStoragePlugin
    secureStorageAvailable = true
    return secureStorageModule
  } catch {
    secureStorageAvailable = false
    return null
  }
}

export async function secureGet(key: string): Promise<string | null> {
  if (isNative) {
    // Try secure storage first
    const secure = await getSecureStorage()
    if (secure) {
      try {
        const result = await secure.get({ key })
        return result.value
      } catch { /* fall through */ }
    }

    // Capacitor Preferences (always available on native)
    try {
      const { value } = await Preferences.get({ key })
      if (value) return value
    } catch { /* fall through */ }
  }

  // Browser: localStorage
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

export async function secureSet(key: string, value: string): Promise<void> {
  if (isNative) {
    const secure = await getSecureStorage()
    if (secure) {
      try { await secure.set({ key, value }); return } catch { /* fall through */ }
    }

    try { await Preferences.set({ key, value }); return } catch { /* fall through */ }
  }

  try {
    localStorage.setItem(key, value)
  } catch {
    console.warn('[SecureStorage] All storage backends failed for set:', key)
  }
}

export async function secureRemove(key: string): Promise<void> {
  if (isNative) {
    const secure = await getSecureStorage()
    if (secure) { try { await secure.remove({ key }) } catch { /* ignore */ } }

    try { await Preferences.remove({ key }) } catch { /* ignore */ }
  }

  try { localStorage.removeItem(key) } catch { /* ignore */ }
}
