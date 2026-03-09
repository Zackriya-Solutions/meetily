'use client'

import React, { useState } from 'react'
import { resetPassword, forgotPassword } from '@/services/authService'

interface ResetPasswordFormProps {
  email: string
  onSwitchToLogin: () => void
  onSuccess: () => void
}

export function ResetPasswordForm({ email, onSwitchToLogin, onSuccess }: ResetPasswordFormProps) {
  const [code, setCode] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState('')
  const [resending, setResending] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')

    if (newPassword !== confirmPassword) {
      setError('Passwords do not match')
      return
    }

    setLoading(true)
    try {
      await resetPassword(email, code, newPassword)
      setSuccess('Password reset successfully. You can now sign in.')
      setTimeout(onSuccess, 1500)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Reset failed')
    } finally {
      setLoading(false)
    }
  }

  const handleResend = async () => {
    setResending(true)
    setError('')
    try {
      await forgotPassword(email)
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
        Enter the 6-digit code sent to <span className="font-medium">{email}</span> and choose a new password.
      </p>

      <div>
        <label htmlFor="reset-code" className="block text-sm font-medium text-gray-700 mb-1">
          Reset Code
        </label>
        <input
          id="reset-code"
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

      <div>
        <label htmlFor="reset-new-password" className="block text-sm font-medium text-gray-700 mb-1">
          New Password
        </label>
        <input
          id="reset-new-password"
          type="password"
          value={newPassword}
          onChange={e => setNewPassword(e.target.value)}
          required
          minLength={8}
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="Min 8 characters"
        />
        <p className="text-xs text-gray-400 mt-1">Min 8 chars, 1 uppercase, 1 lowercase, 1 digit</p>
      </div>

      <div>
        <label htmlFor="reset-confirm-password" className="block text-sm font-medium text-gray-700 mb-1">
          Confirm Password
        </label>
        <input
          id="reset-confirm-password"
          type="password"
          value={confirmPassword}
          onChange={e => setConfirmPassword(e.target.value)}
          required
          minLength={8}
          className="w-full px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          placeholder="Re-enter new password"
        />
      </div>

      {error && <p className="text-sm text-red-600">{error}</p>}
      {success && <p className="text-sm text-green-600">{success}</p>}

      <button
        type="submit"
        disabled={loading || code.length !== 6}
        className="w-full py-2 px-4 bg-blue-600 text-white rounded-md text-sm font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {loading ? 'Resetting...' : 'Reset Password'}
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
