'use client'

import React, { useState } from 'react'
import { useAuth } from '@/contexts/AuthContext'

interface LoginFormProps {
  deviceId: string
  onSwitchToRegister: () => void
  onForgotPassword: () => void
  onNeedsVerification: (email: string) => void
  onSuccess: () => void
}

export function LoginForm({ deviceId, onSwitchToRegister, onForgotPassword, onNeedsVerification, onSuccess }: LoginFormProps) {
  const { login, error, clearError } = useAuth()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    clearError()
    setLoading(true)
    try {
      await login(email, password, deviceId)
      onSuccess()
    } catch (err: unknown) {
      // Check if it's an email verification issue
      if (err instanceof Error && err.message.toLowerCase().includes('not verified')) {
        onNeedsVerification(email)
      }
      // error is set in context
    } finally {
      setLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label htmlFor="login-email" className="block text-sm font-medium text-gray-700 mb-1">
          Email
        </label>
        <input
          id="login-email"
          type="email"
          value={email}
          onChange={e => setEmail(e.target.value)}
          required
          autoFocus
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="you@example.com"
        />
      </div>

      <div>
        <label htmlFor="login-password" className="block text-sm font-medium text-gray-700 mb-1">
          Password
        </label>
        <input
          id="login-password"
          type="password"
          value={password}
          onChange={e => setPassword(e.target.value)}
          required
          minLength={8}
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="Min 8 characters"
        />
      </div>

      <div className="text-right">
        <button type="button" onClick={onForgotPassword} className="text-xs text-blue-600 hover:underline">
          Forgot Password?
        </button>
      </div>

      {error && (
        <p className="text-sm text-red-600">{error}</p>
      )}

      <button
        type="submit"
        disabled={loading}
        className="w-full py-2 px-4 bg-blue-600 text-white rounded-md text-sm font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {loading ? 'Signing in...' : 'Sign In'}
      </button>

      <p className="text-xs text-center text-gray-500">
        Don&apos;t have an account?{' '}
        <button type="button" onClick={onSwitchToRegister} className="text-blue-600 hover:underline">
          Create one
        </button>
      </p>
    </form>
  )
}
