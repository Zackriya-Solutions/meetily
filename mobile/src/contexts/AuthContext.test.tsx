import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import React from 'react'
import { AuthProvider, useAuth } from './AuthContext'

// ── Mock services ──
vi.mock('@/services/authService', () => ({
  login: vi.fn(),
  register: vi.fn(),
  logoutApi: vi.fn(),
  getAccessToken: vi.fn(),
  getMe: vi.fn(),
  refreshTokens: vi.fn(),
  clearTokens: vi.fn(),
}))

vi.mock('@/services/deviceService', () => ({
  getDeviceId: vi.fn().mockResolvedValue('mock-device-id'),
  getPlatform: vi.fn().mockReturnValue('mobile'),
}))

import * as authService from '@/services/authService'
import { getDeviceId, getPlatform } from '@/services/deviceService'

const mockUser = {
  user_id: 'u1',
  email: 'test@example.com',
  display_name: 'Test User',
  devices: [],
}

const mockAuthResponse = {
  access_token: 'at-123',
  refresh_token: 'rt-123',
  user: mockUser,
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <AuthProvider>{children}</AuthProvider>
}

describe('AuthContext', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // Default: no existing session
    vi.mocked(authService.getAccessToken).mockResolvedValue(null)
    vi.mocked(authService.refreshTokens).mockResolvedValue(null)
    vi.mocked(authService.clearTokens).mockResolvedValue(undefined)
    vi.mocked(authService.logoutApi).mockResolvedValue(undefined)
  })

  it('throws when useAuth is used outside AuthProvider', () => {
    expect(() => {
      renderHook(() => useAuth())
    }).toThrow('useAuth must be used within AuthProvider')
  })

  it('has initial state: not authenticated, loading', async () => {
    const { result } = renderHook(() => useAuth(), { wrapper })

    // Initially loading
    expect(result.current.isLoading).toBe(true)
    expect(result.current.user).toBeNull()
    expect(result.current.isAuthenticated).toBe(false)
    expect(result.current.error).toBeNull()

    // After session restoration resolves
    await waitFor(() => {
      expect(result.current.isLoading).toBe(false)
    })
    expect(result.current.isAuthenticated).toBe(false)
  })

  describe('login', () => {
    it('calls authService.login with device info, sets user on success, returns true', async () => {
      vi.mocked(authService.login).mockResolvedValue(mockAuthResponse)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let loginResult: any
      await act(async () => {
        loginResult = await result.current.login('test@example.com', 'password123')
      })

      expect(loginResult).toBe(true)
      expect(authService.login).toHaveBeenCalledWith(
        'test@example.com',
        'password123',
        'mock-device-id',
        'mobile',
      )
      expect(result.current.user).toEqual(mockUser)
      expect(result.current.isAuthenticated).toBe(true)
      expect(result.current.error).toBeNull()
    })

    it("returns 'EMAIL_NOT_VERIFIED' when error code matches", async () => {
      const error = new Error('EMAIL_NOT_VERIFIED') as any
      error.code = 'EMAIL_NOT_VERIFIED'
      vi.mocked(authService.login).mockRejectedValue(error)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let loginResult: any
      await act(async () => {
        loginResult = await result.current.login('test@example.com', 'password123')
      })

      expect(loginResult).toBe('EMAIL_NOT_VERIFIED')
      expect(result.current.error).toBe('Please verify your email before signing in.')
      expect(result.current.user).toBeNull()
      expect(result.current.isAuthenticated).toBe(false)
    })

    it('sets error on failure, returns false', async () => {
      vi.mocked(authService.login).mockRejectedValue(new Error('Invalid credentials'))

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let loginResult: any
      await act(async () => {
        loginResult = await result.current.login('test@example.com', 'wrong')
      })

      expect(loginResult).toBe(false)
      expect(result.current.error).toBe('Invalid credentials')
      expect(result.current.user).toBeNull()
    })

    it('uses fallback error message when error has no message', async () => {
      const error = new Error() as any
      error.message = ''
      vi.mocked(authService.login).mockRejectedValue(error)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let loginResult: any
      await act(async () => {
        loginResult = await result.current.login('a@b.com', 'pw')
      })

      expect(loginResult).toBe(false)
      expect(result.current.error).toBe('Login failed')
    })
  })

  describe('register', () => {
    it('calls authService.register, returns true on success (does NOT set user)', async () => {
      vi.mocked(authService.register).mockResolvedValue(mockAuthResponse)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let registerResult: any
      await act(async () => {
        registerResult = await result.current.register('new@example.com', 'password123', 'New User')
      })

      expect(registerResult).toBe(true)
      expect(authService.register).toHaveBeenCalledWith(
        'new@example.com',
        'password123',
        'mock-device-id',
        'New User',
        'mobile',
      )
      // User should NOT be set (email verification required)
      expect(result.current.user).toBeNull()
      expect(result.current.isAuthenticated).toBe(false)
    })

    it('sets error on failure, returns false', async () => {
      vi.mocked(authService.register).mockRejectedValue(new Error('Email already exists'))

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      let registerResult: any
      await act(async () => {
        registerResult = await result.current.register('existing@example.com', 'password123')
      })

      expect(registerResult).toBe(false)
      expect(result.current.error).toBe('Email already exists')
    })

    it('uses fallback error message when error has no message', async () => {
      const error = new Error() as any
      error.message = ''
      vi.mocked(authService.register).mockRejectedValue(error)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      await act(async () => {
        await result.current.register('a@b.com', 'pw')
      })

      expect(result.current.error).toBe('Registration failed')
    })
  })

  describe('logout', () => {
    it('clears user and calls logoutApi', async () => {
      // Start logged in via session restoration
      vi.mocked(authService.getAccessToken).mockResolvedValue('token-123')
      vi.mocked(authService.getMe).mockResolvedValue(mockUser)

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isAuthenticated).toBe(true))

      await act(async () => {
        await result.current.logout()
      })

      expect(authService.logoutApi).toHaveBeenCalled()
      expect(result.current.user).toBeNull()
      expect(result.current.isAuthenticated).toBe(false)
    })
  })

  describe('clearError', () => {
    it('clears error state', async () => {
      vi.mocked(authService.login).mockRejectedValue(new Error('Some error'))

      const { result } = renderHook(() => useAuth(), { wrapper })
      await waitFor(() => expect(result.current.isLoading).toBe(false))

      await act(async () => {
        await result.current.login('a@b.com', 'pw')
      })
      expect(result.current.error).toBe('Some error')

      act(() => {
        result.current.clearError()
      })
      expect(result.current.error).toBeNull()
    })
  })

  describe('session restoration', () => {
    it('restores session via getAccessToken + getMe', async () => {
      vi.mocked(authService.getAccessToken).mockResolvedValue('token-123')
      vi.mocked(authService.getMe).mockResolvedValue(mockUser)

      const { result } = renderHook(() => useAuth(), { wrapper })

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false)
        expect(result.current.user).toEqual(mockUser)
      })

      expect(result.current.isAuthenticated).toBe(true)
      expect(authService.getAccessToken).toHaveBeenCalled()
      expect(authService.getMe).toHaveBeenCalled()
    })

    it('falls back to refreshTokens when getMe fails', async () => {
      vi.mocked(authService.getAccessToken).mockResolvedValue('expired-token')
      vi.mocked(authService.getMe).mockRejectedValue(new Error('Unauthorized'))
      vi.mocked(authService.refreshTokens).mockResolvedValue(mockAuthResponse)

      const { result } = renderHook(() => useAuth(), { wrapper })

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false)
        expect(result.current.user).toEqual(mockUser)
      })

      expect(result.current.isAuthenticated).toBe(true)
      expect(authService.refreshTokens).toHaveBeenCalled()
    })

    it('clears tokens when both getMe and refreshTokens fail', async () => {
      vi.mocked(authService.getAccessToken).mockResolvedValue('bad-token')
      vi.mocked(authService.getMe).mockRejectedValue(new Error('Unauthorized'))
      vi.mocked(authService.refreshTokens).mockRejectedValue(new Error('Refresh failed'))

      const { result } = renderHook(() => useAuth(), { wrapper })

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false)
      })

      expect(result.current.user).toBeNull()
      expect(authService.clearTokens).toHaveBeenCalled()
    })

    it('stops loading when no token exists', async () => {
      vi.mocked(authService.getAccessToken).mockResolvedValue(null)

      const { result } = renderHook(() => useAuth(), { wrapper })

      await waitFor(() => {
        expect(result.current.isLoading).toBe(false)
      })

      expect(result.current.user).toBeNull()
      expect(authService.getMe).not.toHaveBeenCalled()
    })
  })
})
