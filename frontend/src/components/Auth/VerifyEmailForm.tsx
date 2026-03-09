'use client'

import React, { useState } from 'react'
import { verifyEmail, resendVerification } from '@/services/authService'

interface VerifyEmailFormProps {
  email: string
  onSuccess: () => void
  onSwitchToLogin: () => void
}

export function VerifyEmailForm({ email, onSuccess, onSwitchToLogin }: VerifyEmailFormProps) {
  const [code, setCode] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const [resending, setResending] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      await verifyEmail(email, code)
      setSuccess('Email verified! You can now sign in.')
      setTimeout(onSuccess, 1500)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Verification failed')
    } finally {
      setLoading(false)
    }
  }

  const handleResend = async () => {
    setResending(true)
    setError('')
    try {
      await resendVerification(email)
      setSuccess('A new code has been sent.')
      setTimeout(() => setSuccess(''), 3000)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Could not resend code')
    } finally {
      setResending(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <p className="text-sm text-gray-600">
        We sent a 6-digit verification code to <span className="font-medium">{email}</span>.
        Enter it below to verify your email address.
      </p>

      <div>
        <label htmlFor="verify-code" className="block text-sm font-medium text-gray-700 mb-1">
          Verification Code
        </label>
        <input
          id="verify-code"
          type="text"
          value={code}
          onChange={e => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
          required
          autoFocus
          maxLength={6}
          pattern="\d{6}"
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm tracking-widest text-center font-mono focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="000000"
        />
      </div>

      {error && <p className="text-sm text-red-600">{error}</p>}
      {success && <p className="text-sm text-green-600">{success}</p>}

      <button
        type="submit"
        disabled={loading || code.length !== 6}
        className="w-full py-2 px-4 bg-blue-600 text-white rounded-md text-sm font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {loading ? 'Verifying...' : 'Verify Email'}
      </button>

      <div className="flex items-center justify-between text-xs text-gray-500">
        <button type="button" onClick={onSwitchToLogin} className="text-blue-600 hover:underline">
          Back to Sign In
        </button>
        <button
          type="button"
          onClick={handleResend}
          disabled={resending}
          className="text-blue-600 hover:underline disabled:opacity-50"
        >
          {resending ? 'Sending...' : 'Resend code'}
        </button>
      </div>
    </form>
  )
}
