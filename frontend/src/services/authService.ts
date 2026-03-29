/**
 * Authentication service for communicating with the cloud API.
 *
 * All auth requests go to the CLOUD_API_URL, not the local backend.
 */

// Use Tauri invoke when available, fall back to localStorage for browser dev
const isTauri = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__

// Lazy-loaded Tauri invoke — awaited on first use to avoid race conditions
let _tauriInvokePromise: Promise<typeof import('@tauri-apps/api/core').invoke> | null = null
async function getTauriInvoke() {
  if (!isTauri) return null
  if (!_tauriInvokePromise) {
    _tauriInvokePromise = import('@tauri-apps/api/core').then(m => m.invoke).catch(() => null as any)
  }
  return _tauriInvokePromise
}

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

// ── Token management (Tauri secure store with browser fallback) ─────

export async function getAccessToken(): Promise<string | null> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    try { return await invoke<string | null>('auth_get_access_token') } catch { return null }
  }
  return localStorage.getItem('auth_access_token')
}

export async function getRefreshToken(): Promise<string | null> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    try { return await invoke<string | null>('auth_get_refresh_token') } catch { return null }
  }
  return localStorage.getItem('auth_refresh_token')
}

export async function saveTokens(accessToken: string, refreshToken: string): Promise<void> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    await invoke('auth_save_tokens', { accessToken, refreshToken })
  } else {
    localStorage.setItem('auth_access_token', accessToken)
    localStorage.setItem('auth_refresh_token', refreshToken)
  }
}

export async function clearTokens(): Promise<void> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    await invoke('auth_clear_tokens')
  } else {
    localStorage.removeItem('auth_access_token')
    localStorage.removeItem('auth_refresh_token')
  }
}

async function saveAuthUserId(userId: string): Promise<void> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    await invoke('auth_save_user_id', { userId })
  } else {
    localStorage.setItem('auth_user_id', userId)
  }
}

export async function getAuthUserId(): Promise<string | null> {
  const invoke = await getTauriInvoke()
  if (invoke) {
    try { return await invoke<string | null>('auth_get_user_id') } catch { return null }
  }
  return localStorage.getItem('auth_user_id')
}

// ── Error extraction (handles Pydantic validation arrays) ───────────

function extractErrorMessage(detail: unknown, fallback: string): string {
  if (typeof detail === 'string') return detail
  if (Array.isArray(detail)) return detail.map((d: any) => d.msg || d.message || String(d)).join('; ')
  return fallback
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
  org_id: string | null
  org_role: string | null
  org_name: string | null
  org_brand_template_id: string | null
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
  inviteCode?: string,
): Promise<AuthResponse> {
  const res = await fetch(`${getBaseUrl()}/api/auth/register`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      email,
      password,
      device_id: deviceId,
      display_name: displayName || null,
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
): Promise<AuthResponse> {
  const res = await fetch(`${getBaseUrl()}/api/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password, device_id: deviceId }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Login failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Login failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Request failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Reset failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Change failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Verification failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Request failed'))
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
    throw new Error(extractErrorMessage(err.detail, 'Update failed'))
  }
  return res.json()
}

export async function deactivateAccount(): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/deactivate', { method: 'POST' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Deactivation failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Deactivation failed'))
  }
  return res.json()
}

export async function deleteAccount(): Promise<{ message: string }> {
  const res = await authFetch('/api/auth/account', { method: 'DELETE' })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Deletion failed' }))
    throw new Error(extractErrorMessage(err.detail, 'Deletion failed'))
  }
  return res.json()
}

// ── Organisation ──────────────────────────────────────────────────

export async function createInvite(
  email?: string,
  role: string = 'member',
): Promise<{ code: string; org_name: string; expires_at: string }> {
  const res = await authFetch('/api/org/invites', {
    method: 'POST',
    body: JSON.stringify({ email: email || null, role }),
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ detail: 'Failed to create invite' }))
    throw new Error(extractErrorMessage(err.detail, 'Failed to create invite'))
  }
  return res.json()
}
