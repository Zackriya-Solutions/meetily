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

/**
 * Secure storage abstraction layer.
 *
 * On native (Capacitor): uses Keychain/Keystore via secure-storage plugin,
 * then Capacitor Preferences as fallback.
 * On web/browser: uses localStorage directly (Capacitor plugins may hang).
 */

const isNative = typeof window !== 'undefined' && !!(window as any).Capacitor?.isNativePlatform?.()

let secureStorageModule: any = null
let secureStorageAvailable: boolean | null = null
let preferencesModule: any = null

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

async function getPreferences(): Promise<any | null> {
  if (!isNative) return null
  if (preferencesModule) return preferencesModule

  try {
    const mod = await import('@capacitor/preferences')
    preferencesModule = mod.Preferences
    return preferencesModule
  } catch {
    return null
  }
}

export async function secureGet(key: string): Promise<string | null> {
  // Native: try secure storage, then Preferences
  if (isNative) {
    const secure = await getSecureStorage()
    if (secure) {
      try {
        const result = await secure.get({ key })
        return result.value
      } catch { /* fall through */ }
    }

    const prefs = await getPreferences()
    if (prefs) {
      try {
        const { value } = await prefs.get({ key })
        if (value) return value
      } catch { /* fall through */ }
    }
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

    const prefs = await getPreferences()
    if (prefs) {
      try { await prefs.set({ key, value }); return } catch { /* fall through */ }
    }
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

    const prefs = await getPreferences()
    if (prefs) { try { await prefs.remove({ key }) } catch { /* ignore */ } }
  }

  try { localStorage.removeItem(key) } catch { /* ignore */ }
}
