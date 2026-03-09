/**
 * Authentication service for communicating with the cloud API.
 *
 * All auth requests go to the CLOUD_API_URL, not the local backend.
 */

import { invoke } from '@tauri-apps/api/core'

// Cloud API URL — configurable via environment or fetched from local backend
let cloudApiUrl: string = process.env.NEXT_PUBLIC_CLOUD_API_URL || ''

export async function initCloudApiUrl(): Promise<void> {
  if (cloudApiUrl) return
  try {
    const res = await fetch('http://localhost:5167/api/config')
    if (res.ok) {
      const data = await res.json()
      if (data.cloud_api_url) {
        cloudApiUrl = data.cloud_api_url
      }
    }
  } catch {
    console.warn('[AuthService] Could not fetch cloud API URL from local backend')
  }
}

function getBaseUrl(): string {
  return cloudApiUrl || 'http://localhost:5167'
}

// ── Token management (Tauri secure store) ───────────────────────────

export async function getAccessToken(): Promise<string | null> {
  try {
    return await invoke<string | null>('auth_get_access_token')
  } catch {
    return null
  }
}

export async function getRefreshToken(): Promise<string | null> {
  try {
    return await invoke<string | null>('auth_get_refresh_token')
  } catch {
    return null
  }
}

export async function saveTokens(accessToken: string, refreshToken: string): Promise<void> {
  await invoke('auth_save_tokens', { accessToken, refreshToken })
}

export async function clearTokens(): Promise<void> {
  await invoke('auth_clear_tokens')
}

async function saveAuthUserId(userId: string): Promise<void> {
  await invoke('auth_save_user_id', { userId })
}

export async function getAuthUserId(): Promise<string | null> {
  try {
    return await invoke<string | null>('auth_get_user_id')
  } catch {
    return null
  }
}

// ── Authenticated fetch helper ──────────────────────────────────────

async function authFetch(path: string, options: RequestInit = {}): Promise<Response> {
  const token = await getAccessToken()
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  }
  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }
  return fetch(`${getBaseUrl()}${path}`, { ...options, headers })
}

// ── Auth API calls ──────────────────────────────────────────────────

export interface UserProfile {
  user_id: string
  email: string
  display_name: string | null
  account_level: string | null
  email_verified: boolean | null
  devices: DeviceSummary[]
}

export interface DeviceSummary {
  device_id: string
  linked_at: string
  platform: string | null
  last_seen: string | null
}

export interface AuthResponse {
  access_token: string
  refresh_token: string
  token_type: string
  user: UserProfile
}

export async function register(
  email: string,
  password: string,
  deviceId: string,
  displayName?: string,
): Promise<AuthResponse> {
  const res = await fetch(`${getBaseUrl()}/api/auth/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password,
      device_id: deviceId,
      display_name: displayName || null,
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
): Promise<AuthResponse> {
  const res = await fetch(`${getBaseUrl()}/api/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password, device_id: deviceId }),
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

  const res = await fetch(`${getBaseUrl()}/api/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  })
  if (!res.ok) {
    // Refresh failed — clear tokens
    await clearTokens()
    return null
  }
  const data: AuthResponse = await res.json()
  await saveTokens(data.access_token, data.refresh_token)
  return data
}

export async function logout(): Promise<void> {
  try {
    await authFetch('/api/auth/logout', { method: 'POST' })
  } catch {
    // Best-effort
  }
  await clearTokens()
}

export async function getMe(): Promise<UserProfile | null> {
  const res = await authFetch('/api/auth/me')
  if (!res.ok) return null
  return res.json()
}

export async function linkDevice(deviceId: string, platform?: string): Promise<void> {
  await authFetch('/api/auth/link-device', {
    method: 'POST',
    body: JSON.stringify({ device_id: deviceId, platform }),
  })
}

// ── Password reset ─────────────────────────────────────────────────

export async function forgotPassword(email: string): Promise<{ message: string }> {
  const res = await fetch(`${getBaseUrl()}/api/auth/forgot-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Request failed' }))
    throw new Error(err.detail || 'Request failed')
  }
  return res.json()
}

export async function resetPassword(
  email: string,
  code: string,
  newPassword: string,
): Promise<{ message: string }> {
  const res = await fetch(`${getBaseUrl()}/api/auth/reset-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, code, new_password: newPassword }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Reset failed' }))
    throw new Error(err.detail || 'Reset failed')
  }
  return res.json()
}

export async function changePassword(
  currentPassword: string,
  newPassword: string,
): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/change-password', {
    method: 'POST',
    body: JSON.stringify({
      current_password: currentPassword,
      new_password: newPassword,
    }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Change failed' }))
    throw new Error(err.detail || 'Change failed')
  }
  return res.json()
}

// ── Email verification ────────────────────────────────────────────

export async function verifyEmail(email: string, code: string): Promise<{ message: string }> {
  const res = await fetch(`${getBaseUrl()}/api/auth/verify-email`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, code }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Verification failed' }))
    throw new Error(err.detail || 'Verification failed')
  }
  return res.json()
}

export async function resendVerification(email: string): Promise<{ message: string }> {
  const res = await fetch(`${getBaseUrl()}/api/auth/resend-verification`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Request failed' }))
    throw new Error(err.detail || 'Request failed')
  }
  return res.json()
}

// ── Account management ────────────────────────────────────────────

export async function updateProfile(displayName: string): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/profile', {
    method: 'PUT',
    body: JSON.stringify({ display_name: displayName }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Update failed' }))
    throw new Error(err.detail || 'Update failed')
  }
  return res.json()
}

export async function deactivateAccount(): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/deactivate', { method: 'POST' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Deactivation failed' }))
    throw new Error(err.detail || 'Deactivation failed')
  }
  return res.json()
}

export async function deleteAccount(): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/account', { method: 'DELETE' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Deletion failed' }))
    throw new Error(err.detail || 'Deletion failed')
  }
  return res.json()
}
