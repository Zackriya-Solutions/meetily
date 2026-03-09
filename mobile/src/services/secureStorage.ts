/**
 * Secure storage abstraction layer.
 *
 * Priority order:
 * 1. @capacitor-community/secure-storage (native Keychain/Keystore)
 * 2. @capacitor/preferences (encrypted on native, cleartext on web)
 * 3. localStorage (web fallback — least secure)
 *
 * Tokens and sensitive data should use this module instead of
 * directly accessing Preferences or localStorage.
 */

import { Preferences } from '@capacitor/preferences'

let secureStorageModule: any = null
let secureStorageAvailable: boolean | null = null

async function getSecureStorage(): Promise<any | null> {
  if (secureStorageAvailable === false) return null
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
  // Try secure storage first (Keychain/Keystore)
  const secure = await getSecureStorage()
  if (secure) {
    try {
      const result = await secure.get({ key })
      return result.value
    } catch {
      // Key doesn't exist in secure storage — fall through
    }
  }

  // Fall back to Capacitor Preferences
  try {
    const { value } = await Preferences.get({ key })
    if (value) return value
  } catch {
    // Not available
  }

  // Final fallback — localStorage
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

export async function secureSet(key: string, value: string): Promise<void> {
  const secure = await getSecureStorage()
  if (secure) {
    try {
      await secure.set({ key, value })
      return
    } catch {
      // Fall through
    }
  }

  try {
    await Preferences.set({ key, value })
    return
  } catch {
    // Fall through
  }

  try {
    localStorage.setItem(key, value)
  } catch {
    console.warn('[SecureStorage] All storage backends failed for set:', key)
  }
}

export async function secureRemove(key: string): Promise<void> {
  // Remove from all backends to ensure cleanup
  const secure = await getSecureStorage()
  if (secure) {
    try { await secure.remove({ key }) } catch { /* ignore */ }
  }

  try { await Preferences.remove({ key }) } catch { /* ignore */ }

  try { localStorage.removeItem(key) } catch { /* ignore */ }
}
