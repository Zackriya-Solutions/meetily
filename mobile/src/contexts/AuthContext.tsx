'use client'

import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react'
import { UserProfile } from '@/types'
import * as authService from '@/services/authService'
import { getDeviceId, getPlatform } from '@/services/deviceService'

interface AuthContextValue {
  user: UserProfile | null
  isAuthenticated: boolean
  isLoading: boolean
  error: string | null
  login: (email: string, password: string) => Promise<boolean>
  register: (email: string, password: string, displayName?: string) => Promise<boolean>
  logout: () => Promise<void>
  clearError: () => void
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within AuthProvider')
  return ctx
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<UserProfile | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const isAuthenticated = !!user

  // Schedule token refresh every 12 minutes
  const scheduleRefresh = useCallback(() => {
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    refreshTimerRef.current = setTimeout(async () => {
      try {
        const result = await authService.refreshTokens()
        if (result) {
          setUser(result.user)
          scheduleRefresh()
        } else {
          setUser(null)
        }
      } catch {
        setUser(null)
      }
    }, 12 * 60 * 1000) // 12 minutes
  }, [])

  // Restore session on mount
  useEffect(() => {
    const restore = async () => {
      try {
        const token = await authService.getAccessToken()
        if (!token) {
          setIsLoading(false)
          return
        }
        const profile = await authService.getMe()
        setUser(profile)
        scheduleRefresh()
      } catch {
        // Try refresh token
        try {
          const result = await authService.refreshTokens()
          if (result) {
            setUser(result.user)
            scheduleRefresh()
          }
        } catch {
          await authService.clearTokens()
        }
      } finally {
        setIsLoading(false)
      }
    }
    restore()

    return () => {
      if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    }
  }, [scheduleRefresh])

  const login = useCallback(async (email: string, password: string): Promise<boolean> => {
    setError(null)
    setIsLoading(true)
    try {
      const deviceId = await getDeviceId()
      const platform = getPlatform()
      const result = await authService.login(email, password, deviceId, platform)
      setUser(result.user)
      scheduleRefresh()
      return true
    } catch (e: any) {
      setError(e.message || 'Login failed')
      return false
    } finally {
      setIsLoading(false)
    }
  }, [scheduleRefresh])

  const register = useCallback(async (
    email: string,
    password: string,
    displayName?: string,
  ): Promise<boolean> => {
    setError(null)
    setIsLoading(true)
    try {
      const deviceId = await getDeviceId()
      const platform = getPlatform()
      const result = await authService.register(email, password, deviceId, displayName, platform)
      setUser(result.user)
      scheduleRefresh()
      return true
    } catch (e: any) {
      setError(e.message || 'Registration failed')
      return false
    } finally {
      setIsLoading(false)
    }
  }, [scheduleRefresh])

  const logout = useCallback(async () => {
    if (refreshTimerRef.current) clearTimeout(refreshTimerRef.current)
    await authService.logoutApi()
    setUser(null)
  }, [])

  const clearError = useCallback(() => setError(null), [])

  return (
    <AuthContext.Provider
      value={{ user, isAuthenticated, isLoading, error, login, register, logout, clearError }}
    >
      {children}
    </AuthContext.Provider>
  )
}
