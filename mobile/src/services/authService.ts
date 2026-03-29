/**
 * Auth service for mobile — adapted from desktop's authService.ts.
 * Replaces Tauri invoke() with Capacitor SecureStorage for token persistence.
 */

import { UserProfile, AuthResponse, DeviceSummary } from '@/types'
import { secureGet, secureSet, secureRemove } from './secureStorage'

import { config } from './config'

function getBaseUrl(): string {
  return config.apiUrl
}

// ── Error extraction (handles Pydantic validation arrays) ──

function extractErrorMessage(detail: unknown, fallback: string): string {
  if (typeof detail === 'string') return detail
  if (Array.isArray(detail)) return detail.map((d: any) => d.msg || d.message || String(d)).join('; ')
  return fallback
}

// ── Token storage (Secure Storage → Preferences → localStorage) ──

export async function getAccessToken(): Promise<string | null> {
  return secureGet('access_token')
}

export async function getRefreshToken(): Promise<string | null> {
  return secureGet('refresh_token')
}

export async function saveTokens(accessToken: string, refreshToken: string): Promise<void> {
  await secureSet('access_token', accessToken)
  await secureSet('refresh_token', refreshToken)
}

export async function clearTokens(): Promise<void> {
  await secureRemove('access_token')
  await secureRemove('refresh_token')
  await secureRemove('auth_user_id')
}

export async function saveAuthUserId(userId: string): Promise<void> {
  await secureSet('auth_user_id', userId)
}

export async function getAuthUserId(): Promise<string | null> {
  return secureGet('auth_user_id')
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
  inviteCode?: string,
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
      invite_code: inviteCode,
    }),
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Registration failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Registration failed'))
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
    const detail = extractErrorMessage(err.detail, 'Login failed')
    if (detail === 'EMAIL_NOT_VERIFIED') {
      const error = new Error(detail)
      ;(error as any).code = 'EMAIL_NOT_VERIFIED'
      ;(error as any).email = email
      throw error
    }
    throw new Error(detail)
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

// ── Forgot / Reset Password ──

export async function forgotPassword(email: string): Promise<void> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/forgot-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Request failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Request failed'))
  }
}

export async function resetPassword(email: string, code: string, newPassword: string): Promise<void> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/reset-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, code, new_password: newPassword }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Reset failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Reset failed'))
  }
}

// ── Email Verification ──

export async function verifyEmail(email: string, code: string): Promise<void> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/verify-email`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, code }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Verification failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Verification failed'))
  }
}

export async function resendVerification(email: string): Promise<void> {
  const baseUrl = getBaseUrl()
  const res = await fetch(`${baseUrl}/api/auth/resend-verification`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Request failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Request failed'))
  }
}

// ── Change Password ──

export async function changePassword(currentPassword: string, newPassword: string): Promise<void> {
  const res = await authFetch('/api/auth/change-password', {
    method: 'POST',
    body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Failed to change password' }))
    throw new Error(extractErrorMessage(err.detail, 'Failed to change password'))
  }
}

// ── Profile Update ──

export async function updateProfile(displayName: string): Promise<void> {
  const res = await authFetch('/api/auth/profile', {
    method: 'PUT',
    body: JSON.stringify({ display_name: displayName }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Failed to update profile' }))
    throw new Error(extractErrorMessage(err.detail, 'Failed to update profile'))
  }
}

// ── Account Management ──

export async function deactivateAccount(): Promise<void> {
  const res = await authFetch('/api/auth/deactivate', { method: 'POST' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Failed to deactivate account' }))
    throw new Error(extractErrorMessage(err.detail, 'Failed to deactivate account'))
  }
  await clearTokens()
}

export async function deleteAccount(): Promise<void> {
  const res = await authFetch('/api/auth/account', { method: 'DELETE' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Failed to delete account' }))
    throw new Error(extractErrorMessage(err.detail, 'Failed to delete account'))
  }
  await clearTokens()
}

// ── Device Management ──

export async function getDevices(): Promise<DeviceSummary[]> {
  const res = await authFetch('/api/auth/devices')
  if (!res.ok) throw new Error('Failed to get devices')
  return res.json()
}
