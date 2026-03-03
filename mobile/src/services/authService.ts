/**
 * Auth service for mobile — adapted from desktop's authService.ts.
 * Replaces Tauri invoke() with Capacitor SecureStorage for token persistence.
 */

import { Preferences } from '@capacitor/preferences'
import { UserProfile, AuthResponse } from '@/types'

// Cloud API URL — mobile always talks directly to the cloud
const CLOUD_API_URL = process.env.NEXT_PUBLIC_CLOUD_API_URL || ''

function getBaseUrl(): string {
  return CLOUD_API_URL
}

// ── Token storage (Capacitor Preferences — encrypted on device) ──

export async function getAccessToken(): Promise<string | null> {
  try {
    const { value } = await Preferences.get({ key: 'access_token' })
    return value
  } catch {
    // Fallback for web/dev mode
    return localStorage.getItem('access_token')
  }
}

export async function getRefreshToken(): Promise<string | null> {
  try {
    const { value } = await Preferences.get({ key: 'refresh_token' })
    return value
  } catch {
    return localStorage.getItem('refresh_token')
  }
}

export async function saveTokens(accessToken: string, refreshToken: string): Promise<void> {
  try {
    await Preferences.set({ key: 'access_token', value: accessToken })
    await Preferences.set({ key: 'refresh_token', value: refreshToken })
  } catch {
    localStorage.setItem('access_token', accessToken)
    localStorage.setItem('refresh_token', refreshToken)
  }
}

export async function clearTokens(): Promise<void> {
  try {
    await Preferences.remove({ key: 'access_token' })
    await Preferences.remove({ key: 'refresh_token' })
    await Preferences.remove({ key: 'auth_user_id' })
  } catch {
    localStorage.removeItem('access_token')
    localStorage.removeItem('refresh_token')
    localStorage.removeItem('auth_user_id')
  }
}

export async function saveAuthUserId(userId: string): Promise<void> {
  try {
    await Preferences.set({ key: 'auth_user_id', value: userId })
  } catch {
    localStorage.setItem('auth_user_id', userId)
  }
}

export async function getAuthUserId(): Promise<string | null> {
  try {
    const { value } = await Preferences.get({ key: 'auth_user_id' })
    return value
  } catch {
    return localStorage.getItem('auth_user_id')
  }
}

// ── Authenticated fetch helper ──

export async function authFetch(path: string, options: RequestInit = {}): Promise<Response> {
  const token = await getAccessToken()
  const baseUrl = getBaseUrl()

  return fetch(`${baseUrl}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  })
}

// ── API calls (identical to desktop — just fetch to cloud) ──

export async function register(
  email: string,
  password: string,
  deviceId: string,
  displayName?: string,
  platform?: string,
): Promise<AuthResponse> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password,
      device_id: deviceId,
      display_name: displayName,
      platform: platform || 'mobile',
    }),
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Registration failed' }))
    throw new Error(err.detail || 'Registration failed')
  }

  const data: AuthResponse = await res.json()
  await saveTokens(data.access_token, data.refresh_token)
  await saveAuthUserId(data.user.user_id)
  return data
}

export async function login(
  email: string,
  password: string,
  deviceId: string,
  platform?: string,
): Promise<AuthResponse> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password,
      device_id: deviceId,
      platform: platform || 'mobile',
    }),
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Login failed' }))
    throw new Error(err.detail || 'Login failed')
  }

  const data: AuthResponse = await res.json()
  await saveTokens(data.access_token, data.refresh_token)
  await saveAuthUserId(data.user.user_id)
  return data
}

export async function refreshTokens(): Promise<AuthResponse | null> {
  const refreshToken = await getRefreshToken()
  if (!refreshToken) return null

  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  })

  if (!res.ok) {
    await clearTokens()
    return null
  }

  const data: AuthResponse = await res.json()
  await saveTokens(data.access_token, data.refresh_token)
  return data
}

export async function logoutApi(): Promise<void> {
  try {
    const token = await getAccessToken()
    if (token) {
      const baseUrl = getBaseUrl()
      await fetch(`${baseUrl}/api/auth/logout`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
      })
    }
  } catch {
    // Best effort — clear tokens regardless
  }
  await clearTokens()
}

export async function getMe(): Promise<UserProfile> {
  const res = await authFetch('/api/auth/me')
  if (!res.ok) throw new Error('Failed to get profile')
  return res.json()
}

export async function linkDevice(deviceId: string, platform: string): Promise<void> {
  const res = await authFetch('/api/auth/link-device', {
    method: 'POST',
    body: JSON.stringify({ device_id: deviceId, platform }),
  })
  if (!res.ok) throw new Error('Failed to link device')
}
