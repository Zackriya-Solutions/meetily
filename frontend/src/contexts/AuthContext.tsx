'use client'

import React, { createContext, useContext, useEffect, useState, useCallback, useRef } from 'react'
import {
  UserProfile,
  login as apiLogin,
  register as apiRegister,
  logout as apiLogout,
  refreshTokens,
  getMe,
  getAccessToken,
  initCloudApiUrl,
} from '@/services/authService'
import { toast } from 'sonner'
import Analytics from '@/lib/analytics'

interface AuthContextType {
  user: UserProfile | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (email: string, password: string, deviceId: string) => Promise<void>
  register: (email: string, password: string, deviceId: string, displayName?: string) => Promise<void>
  logout: () => Promise<void>
  error: string | null
  clearError: () => void
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<UserProfile | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Auto-refresh access token before expiry (every 12 minutes for 15-min tokens)
  const scheduleRefresh = useCallback(() => {
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    refreshTimerRef.current = setTimeout(async () => {
      try {
        const result = await refreshTokens()
        if (result) {
          setUser(result.user)
          scheduleRefresh()
        } else {
          setUser(prev => {
            if (prev) toast.info('Session expired. Please sign in again.')
            return null
          })
        }
      } catch {
        console.warn('[Auth] Token refresh failed')
        setUser(prev => {
          if (prev) toast.info('Session expired. Please sign in again.')
          return null
        })
      }
    }, 12 * 60 * 1000) // 12 minutes
  }, [])

  // Restore session on mount
  useEffect(() => {
    let cancelled = false

    async function restoreSession() {
      try {
        await initCloudApiUrl()
        const token = await getAccessToken()
        if (!token) {
          setIsLoading(false)
          return
        }

        // Try to get user profile with existing token
        const profile = await getMe()
        if (!cancelled) {
          if (profile) {
            setUser(profile)
            scheduleRefresh()
            // Re-identify restored user in analytics
            Analytics.identify(profile.user_id, {
              email: profile.email,
              account_level: profile.account_level || 'free',
            }).catch(() => {})
          } else {
            // Token expired — try refresh
            const result = await refreshTokens()
            if (result) {
              setUser(result.user)
              scheduleRefresh()
            }
          }
        }
      } catch {
        console.warn('[Auth] Session restore failed')
      } finally {
        if (!cancelled) setIsLoading(false)
      }
    }

    restoreSession()
    return () => { cancelled = true }
  }, [scheduleRefresh])

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    }
  }, [])

  const login = useCallback(async (email: string, password: string, deviceId: string) => {
    setError(null)
    try {
      const result = await apiLogin(email, password, deviceId)
      setUser(result.user)
      scheduleRefresh()
      // Link auth user to analytics
      Analytics.identify(result.user.user_id, {
        email: result.user.email,
        account_level: result.user.account_level || 'free',
      }).catch(() => {})
    } catch (e: any) {
      const msg = e?.message || 'Login failed'
      setError(msg)
      throw e
    }
  }, [scheduleRefresh])

  const register = useCallback(async (email: string, password: string, deviceId: string, displayName?: string) => {
    setError(null)
    try {
      const result = await apiRegister(email, password, deviceId, displayName)
      setUser(result.user)
      scheduleRefresh()
      // Link auth user to analytics
      Analytics.identify(result.user.user_id, {
        email: result.user.email,
        account_level: result.user.account_level || 'free',
      }).catch(() => {})
    } catch (e: any) {
      const msg = e?.message || 'Registration failed'
      setError(msg)
      throw e
    }
  }, [scheduleRefresh])

  const logout = useCallback(async () => {
    await apiLogout()
    setUser(null)
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
  }, [])

  const clearError = useCallback(() => setError(null), [])

  return (
    <AuthContext.Provider
      value={{
        user,
        isAuthenticated: !!user,
        isLoading,
        login,
        register,
        logout,
        error,
        clearError,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth(): AuthContextType {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within AuthProvider')
  return ctx
}
