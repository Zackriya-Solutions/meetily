'use client'

import React, { useState } from 'react'
import { forgotPassword } from '@/services/authService'

interface ForgotPasswordFormProps {
  onSwitchToLogin: () => void
  onCodeSent: (email: string) => void
}

export function ForgotPasswordForm({ onSwitchToLogin, onCodeSent }: ForgotPasswordFormProps) {
  const [email, setEmail] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      await forgotPassword(email)
      onCodeSent(email)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Request failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <p className="text-sm text-gray-600">
        Enter your email address and we&apos;ll send you a code to reset your password.
      </p>

      <div>
        <label htmlFor="forgot-email" className="block text-sm font-medium text-gray-700 mb-1">
          Email
        </label>
        <input
          id="forgot-email"
          type="email"
          value={email}
          onChange={e => setEmail(e.target.value)}
          required
          autoFocus
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="you@example.com"
        />
      </div>

      {error && (
        <p className="text-sm text-red-600">{error}</p>
      )}

      <button
        type="submit"
        disabled={loading}
        className="w-full py-2 px-4 bg-blue-600 text-white rounded-md text-sm font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {loading ? 'Sending...' : 'Send Reset Code'}
      </button>

      <p className="text-xs text-center text-gray-500">
        <button type="button" onClick={onSwitchToLogin} className="text-blue-600 hover:underline">
          Back to Sign In
        </button>
      </p>
    </form>
  )
}
