/**
 * Biometric authentication service.
 *
 * Uses @aparajita/capacitor-biometric-auth for native Face ID / Touch ID / Fingerprint.
 * Falls back gracefully when biometrics are unavailable (web, unsupported devices).
 *
 * Usage: call `checkBiometricOnResume()` from the app resume handler
 * to require biometric unlock when the app returns from background.
 */

import { Preferences } from '@capacitor/preferences'

const BIOMETRIC_ENABLED_KEY = 'biometric_enabled'

let biometricModule: any = null
let biometricAvailable: boolean | null = null

async function getBiometricPlugin(): Promise<any | null> {
  if (biometricAvailable === false) return null
  if (biometricModule) return biometricModule

  try {
    // Try the community biometric plugin
    const mod = await import('@aparajita/capacitor-biometric-auth')
    biometricModule = mod.BiometricAuth
    biometricAvailable = true
    return biometricModule
  } catch {
    biometricAvailable = false
    return null
  }
}

/**
 * Check if biometric authentication is available on this device.
 */
export async function isBiometricAvailable(): Promise<boolean> {
  const plugin = await getBiometricPlugin()
  if (!plugin) return false

  try {
    const result = await plugin.checkBiometry()
    return result.isAvailable
  } catch {
    return false
  }
}

/**
 * Get the type of biometric available (e.g., "Face ID", "Touch ID", "Fingerprint").
 */
export async function getBiometricType(): Promise<string> {
  const plugin = await getBiometricPlugin()
  if (!plugin) return 'Biometrics'

  try {
    const result = await plugin.checkBiometry()
    // Map biometry type to user-friendly name
    switch (result.biometryType) {
      case 1: return 'Touch ID'
      case 2: return 'Face ID'
      case 3: return 'Fingerprint'
      case 4: return 'Face Recognition'
      case 5: return 'Iris'
      default: return 'Biometrics'
    }
  } catch {
    return 'Biometrics'
  }
}

/**
 * Prompt the user for biometric authentication.
 * Returns true if authenticated, false if cancelled or failed.
 */
export async function authenticateWithBiometrics(reason?: string): Promise<boolean> {
  const plugin = await getBiometricPlugin()
  if (!plugin) return true // Pass through if unavailable

  try {
    await plugin.authenticate({
      reason: reason || 'Verify your identity to access IQ:capture',
      cancelTitle: 'Cancel',
      allowDeviceCredential: true, // Allow PIN/passcode fallback
    })
    return true
  } catch {
    return false
  }
}

/**
 * Check if the user has enabled biometric lock for the app.
 */
export async function isBiometricEnabled(): Promise<boolean> {
  try {
    const { value } = await Preferences.get({ key: BIOMETRIC_ENABLED_KEY })
    return value === 'true'
  } catch {
    return false
  }
}

/**
 * Enable or disable biometric lock.
 */
export async function setBiometricEnabled(enabled: boolean): Promise<void> {
  await Preferences.set({ key: BIOMETRIC_ENABLED_KEY, value: enabled ? 'true' : 'false' })
}

/**
 * Called when the app resumes from background.
 * If biometric is enabled, prompts for authentication.
 * Returns true if the user should proceed, false if locked out.
 */
export async function checkBiometricOnResume(): Promise<boolean> {
  const enabled = await isBiometricEnabled()
  if (!enabled) return true

  const available = await isBiometricAvailable()
  if (!available) return true

  return authenticateWithBiometrics('Unlock IQ:capture')
}
