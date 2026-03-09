import { describe, it, expect, vi, beforeEach } from 'vitest'

// Shared store accessible to both the mock and the tests
const _store: Record<string, string> = {}

// We need to mock secureStorage so authService token operations work predictably
vi.mock('./secureStorage', () => {
  return {
    secureGet: vi.fn((key: string) => Promise.resolve(_store[key] ?? null)),
    secureSet: vi.fn((key: string, value: string) => {
      _store[key] = value
      return Promise.resolve()
    }),
    secureRemove: vi.fn((key: string) => {
      delete _store[key]
      return Promise.resolve()
    }),
  }
})

// Mock config to have a deterministic API URL
vi.mock('./config', () => ({
  config: {
    apiUrl: 'https://api.test.example.com',
    environment: 'production',
    version: '0.1.0',
    usageFlushInterval: 60000,
    syncInterval: 60000,
    transcriptionPollInterval: 3000,
  },
}))

import {
  getAccessToken,
  saveTokens,
  clearTokens,
  getRefreshToken,
  getAuthUserId,
  saveAuthUserId,
  register,
  login,
  refreshTokens,
  logoutApi,
  getMe,
  linkDevice,
  forgotPassword,
  resetPassword,
  verifyEmail,
  resendVerification,
  changePassword,
  updateProfile,
  deactivateAccount,
  deleteAccount,
  getDevices,
  authFetch,
} from './authService'
import { secureGet, secureSet, secureRemove } from './secureStorage'

const mockFetch = global.fetch as ReturnType<typeof vi.fn>

function mockFetchOk(data: any) {
  mockFetch.mockResolvedValueOnce({
    ok: true,
    json: () => Promise.resolve(data),
    status: 200,
  })
}

function mockFetchError(status: number, body: any) {
  mockFetch.mockResolvedValueOnce({
    ok: false,
    json: () => Promise.resolve(body),
    status,
  })
}

const fakeAuthResponse = {
  access_token: 'at-123',
  refresh_token: 'rt-456',
  user: {
    user_id: 'user-1',
    email: 'test@example.com',
    display_name: 'Test User',
    devices: [],
  },
}

describe('authService', () => {
  beforeEach(() => {
    // Clear the shared store
    for (const key of Object.keys(_store)) {
      delete _store[key]
    }
    vi.clearAllMocks()
  })

  describe('token management', () => {
    it('saveTokens stores access and refresh tokens via secureStorage', async () => {
      await saveTokens('access-1', 'refresh-1')
      expect(secureSet).toHaveBeenCalledWith('access_token', 'access-1')
      expect(secureSet).toHaveBeenCalledWith('refresh_token', 'refresh-1')
    })

    it('getAccessToken retrieves from secureStorage', async () => {
      await saveTokens('at', 'rt')
      const token = await getAccessToken()
      expect(secureGet).toHaveBeenCalledWith('access_token')
      expect(token).toBe('at')
    })

    it('getRefreshToken retrieves from secureStorage', async () => {
      await saveTokens('at', 'rt')
      const token = await getRefreshToken()
      expect(secureGet).toHaveBeenCalledWith('refresh_token')
      expect(token).toBe('rt')
    })

    it('clearTokens removes all auth keys', async () => {
      await clearTokens()
      expect(secureRemove).toHaveBeenCalledWith('access_token')
      expect(secureRemove).toHaveBeenCalledWith('refresh_token')
      expect(secureRemove).toHaveBeenCalledWith('auth_user_id')
    })

    it('saveAuthUserId and getAuthUserId work', async () => {
      await saveAuthUserId('uid-1')
      expect(secureSet).toHaveBeenCalledWith('auth_user_id', 'uid-1')
      const uid = await getAuthUserId()
      expect(secureGet).toHaveBeenCalledWith('auth_user_id')
      expect(uid).toBe('uid-1')
    })
  })

  describe('register()', () => {
    it('calls correct endpoint, saves tokens, returns data', async () => {
      mockFetchOk(fakeAuthResponse)

      const result = await register('test@example.com', 'pass123', 'dev-1', 'Test User')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/register',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            email: 'test@example.com',
            password: 'pass123',
            device_id: 'dev-1',
            display_name: 'Test User',
            platform: 'mobile',
          }),
        }),
      )
      expect(secureSet).toHaveBeenCalledWith('access_token', 'at-123')
      expect(secureSet).toHaveBeenCalledWith('refresh_token', 'rt-456')
      expect(secureSet).toHaveBeenCalledWith('auth_user_id', 'user-1')
      expect(result).toEqual(fakeAuthResponse)
    })

    it('throws on error response', async () => {
      mockFetchError(400, { detail: 'Email already registered' })

      await expect(register('x@x.com', 'p', 'd-1')).rejects.toThrow('Email already registered')
    })

    it('uses default platform "mobile" when not specified', async () => {
      mockFetchOk(fakeAuthResponse)
      await register('a@b.com', 'p', 'd-1')

      const body = JSON.parse((mockFetch.mock.calls[0][1] as any).body)
      expect(body.platform).toBe('mobile')
    })
  })

  describe('login()', () => {
    it('calls correct endpoint and saves tokens', async () => {
      mockFetchOk(fakeAuthResponse)

      const result = await login('test@example.com', 'pass123', 'dev-1')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/login',
        expect.objectContaining({ method: 'POST' }),
      )
      expect(secureSet).toHaveBeenCalledWith('access_token', 'at-123')
      expect(result.user.user_id).toBe('user-1')
    })

    it('detects EMAIL_NOT_VERIFIED error code', async () => {
      mockFetchError(403, { detail: 'EMAIL_NOT_VERIFIED' })

      try {
        await login('test@example.com', 'pass', 'dev-1')
        expect.fail('should have thrown')
      } catch (err: any) {
        expect(err.message).toBe('EMAIL_NOT_VERIFIED')
        expect(err.code).toBe('EMAIL_NOT_VERIFIED')
        expect(err.email).toBe('test@example.com')
      }
    })

    it('throws generic error for other failures', async () => {
      mockFetchError(401, { detail: 'Invalid credentials' })
      await expect(login('x@x.com', 'p', 'd')).rejects.toThrow('Invalid credentials')
    })
  })

  describe('refreshTokens()', () => {
    it('returns null when no refresh token exists', async () => {
      const result = await refreshTokens()
      expect(result).toBeNull()
      expect(mockFetch).not.toHaveBeenCalled()
    })

    it('refreshes and saves new tokens on success', async () => {
      // Set a refresh token in storage
      _store.refresh_token = 'old-rt'

      mockFetchOk(fakeAuthResponse)

      const result = await refreshTokens()
      expect(result).toEqual(fakeAuthResponse)
      expect(secureSet).toHaveBeenCalledWith('access_token', 'at-123')
      expect(secureSet).toHaveBeenCalledWith('refresh_token', 'rt-456')
    })

    it('clears tokens on failure', async () => {
      _store.refresh_token = 'old-rt'

      mockFetchError(401, { detail: 'Invalid token' })

      const result = await refreshTokens()
      expect(result).toBeNull()
      expect(secureRemove).toHaveBeenCalledWith('access_token')
      expect(secureRemove).toHaveBeenCalledWith('refresh_token')
    })
  })

  describe('logoutApi()', () => {
    it('calls logout endpoint and clears tokens', async () => {
      _store.access_token = 'my-token'

      mockFetchOk({})

      await logoutApi()

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/logout',
        expect.objectContaining({
          method: 'POST',
          headers: expect.objectContaining({ Authorization: 'Bearer my-token' }),
        }),
      )
      expect(secureRemove).toHaveBeenCalledWith('access_token')
    })

    it('clears tokens even if API call fails', async () => {
      _store.access_token = 'my-token'

      mockFetch.mockRejectedValueOnce(new Error('network'))

      await logoutApi()
      expect(secureRemove).toHaveBeenCalledWith('access_token')
    })

    it('skips API call if no token exists', async () => {
      await logoutApi()
      // fetch is not called for logout endpoint (may be called 0 times)
      const logoutCalls = mockFetch.mock.calls.filter(
        (c: any) => typeof c[0] === 'string' && c[0].includes('/logout'),
      )
      expect(logoutCalls).toHaveLength(0)
    })
  })

  describe('getMe()', () => {
    it('fetches profile from /api/auth/me', async () => {
      _store.access_token = 'tok'

      const profile = { user_id: 'u1', email: 'a@b.com', display_name: null, devices: [] }
      mockFetchOk(profile)

      const result = await getMe()
      expect(result).toEqual(profile)
      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/me',
        expect.objectContaining({
          headers: expect.objectContaining({ Authorization: 'Bearer tok' }),
        }),
      )
    })

    it('throws on error', async () => {
      mockFetchError(401, {})
      await expect(getMe()).rejects.toThrow('Failed to get profile')
    })
  })

  describe('linkDevice()', () => {
    it('calls /api/auth/link-device with POST', async () => {
      mockFetchOk({})
      await linkDevice('dev-1', 'ios')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/link-device',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ device_id: 'dev-1', platform: 'ios' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(500, {})
      await expect(linkDevice('d', 'p')).rejects.toThrow('Failed to link device')
    })
  })

  describe('forgotPassword()', () => {
    it('calls correct endpoint', async () => {
      mockFetchOk({})
      await forgotPassword('user@test.com')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/forgot-password',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ email: 'user@test.com' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(400, { detail: 'User not found' })
      await expect(forgotPassword('x@x.com')).rejects.toThrow('User not found')
    })
  })

  describe('resetPassword()', () => {
    it('calls correct endpoint with email, code, new_password', async () => {
      mockFetchOk({})
      await resetPassword('u@t.com', '123456', 'newpass')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/reset-password',
        expect.objectContaining({
          body: JSON.stringify({ email: 'u@t.com', code: '123456', new_password: 'newpass' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(400, { detail: 'Invalid code' })
      await expect(resetPassword('u@t.com', 'bad', 'p')).rejects.toThrow('Invalid code')
    })
  })

  describe('verifyEmail()', () => {
    it('calls correct endpoint', async () => {
      mockFetchOk({})
      await verifyEmail('u@t.com', '999')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/verify-email',
        expect.objectContaining({
          body: JSON.stringify({ email: 'u@t.com', code: '999' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(400, { detail: 'Verification failed' })
      await expect(verifyEmail('u@t.com', 'bad')).rejects.toThrow('Verification failed')
    })
  })

  describe('resendVerification()', () => {
    it('calls correct endpoint', async () => {
      mockFetchOk({})
      await resendVerification('u@t.com')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/resend-verification',
        expect.objectContaining({
          body: JSON.stringify({ email: 'u@t.com' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(429, { detail: 'Too many requests' })
      await expect(resendVerification('u@t.com')).rejects.toThrow('Too many requests')
    })
  })

  describe('changePassword()', () => {
    it('uses authFetch to POST to /api/auth/change-password', async () => {
      mockFetchOk({})
      await changePassword('old-pass', 'new-pass')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/change-password',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ current_password: 'old-pass', new_password: 'new-pass' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(400, { detail: 'Wrong current password' })
      await expect(changePassword('bad', 'new')).rejects.toThrow('Wrong current password')
    })
  })

  describe('updateProfile()', () => {
    it('uses authFetch to PUT to /api/auth/profile', async () => {
      mockFetchOk({})
      await updateProfile('New Name')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/profile',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify({ display_name: 'New Name' }),
        }),
      )
    })

    it('throws on failure', async () => {
      mockFetchError(500, {})
      await expect(updateProfile('x')).rejects.toThrow('Failed to update profile')
    })
  })

  describe('deactivateAccount()', () => {
    it('calls /api/auth/deactivate and clears tokens', async () => {
      mockFetchOk({})
      await deactivateAccount()

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/deactivate',
        expect.objectContaining({ method: 'POST' }),
      )
      expect(secureRemove).toHaveBeenCalledWith('access_token')
    })

    it('throws on failure', async () => {
      mockFetchError(500, {})
      await expect(deactivateAccount()).rejects.toThrow('Failed to deactivate account')
    })
  })

  describe('deleteAccount()', () => {
    it('calls /api/auth/account with DELETE and clears tokens', async () => {
      mockFetchOk({})
      await deleteAccount()

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/auth/account',
        expect.objectContaining({ method: 'DELETE' }),
      )
      expect(secureRemove).toHaveBeenCalledWith('access_token')
    })

    it('throws on failure', async () => {
      mockFetchError(500, {})
      await expect(deleteAccount()).rejects.toThrow('Failed to delete account')
    })
  })

  describe('getDevices()', () => {
    it('fetches devices from /api/auth/devices', async () => {
      const devices = [
        { device_id: 'd1', platform: 'ios', linked_at: '2024-01-01', last_seen: '2024-01-02' },
      ]
      mockFetchOk(devices)

      const result = await getDevices()
      expect(result).toEqual(devices)
    })

    it('throws on failure', async () => {
      mockFetchError(401, {})
      await expect(getDevices()).rejects.toThrow('Failed to get devices')
    })
  })

  describe('authFetch()', () => {
    it('includes Authorization header when token exists', async () => {
      _store.access_token = 'bearer-tok'

      mockFetchOk({})
      await authFetch('/api/test')

      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.test.example.com/api/test',
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer bearer-tok',
            'Content-Type': 'application/json',
          }),
        }),
      )
    })

    it('omits Authorization header when no token', async () => {
      mockFetchOk({})
      await authFetch('/api/test')

      const headers = (mockFetch.mock.calls[0][1] as any).headers
      expect(headers.Authorization).toBeUndefined()
    })
  })
})
