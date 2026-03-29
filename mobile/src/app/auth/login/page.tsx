'use client'

import React, { useState } from 'react'
import { useRouter, useSearchParams } from 'next/navigation'
import { useAuth } from '@/contexts/AuthContext'
import Link from 'next/link'

export default function LoginPage() {
  const { login, error, clearError, isLoading } = useAuth()
  const router = useRouter()
  const searchParams = useSearchParams()
  const verified = searchParams.get('verified')
  const reset = searchParams.get('reset')

  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    clearError()
    const result = await login(email, password)
    if (result === true) {
      router.replace('/')
    } else if (result === 'EMAIL_NOT_VERIFIED') {
      router.push(`/auth/verify-email?email=${encodeURIComponent(email)}`)
    }
  }

  return (
    <div className="flex flex-col items-center justify-center min-h-screen px-6 bg-iq-light">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">Welcome back</h1>
        <p className="text-sm text-iq-medium mb-6">Sign in to IQ:capture</p>

        {verified && (
          <div className="status-banner status-banner-success mb-4">
            <span className="text-sm text-iq-green">Email verified! You can now sign in.</span>
          </div>
        )}
        {reset && (
          <div className="status-banner status-banner-success mb-4">
            <span className="text-sm text-iq-green">Password reset! Sign in with your new password.</span>
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Email</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              autoComplete="email"
              className="w-full px-3 py-2.5 border border-iq-light-shade rounded-iq-lg text-sm text-iq-dark bg-white focus:outline-none focus:ring-2 focus:ring-iq-blue focus:border-transparent"
              placeholder="you@example.com"
            />
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="detail-label">Password</label>
              <Link href="/auth/forgot-password" className="text-xs text-iq-blue font-semibold">
                Forgot password?
              </Link>
            </div>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              autoComplete="current-password"
              className="w-full px-3 py-2.5 border border-iq-light-shade rounded-iq-lg text-sm text-iq-dark bg-white focus:outline-none focus:ring-2 focus:ring-iq-blue focus:border-transparent"
              placeholder="Your password"
            />
          </div>

          {error && (
            <div className="status-banner status-banner-danger">
              <span className="text-sm text-iq-red">{error}</span>
            </div>
          )}

          <button
            type="submit"
            disabled={isLoading}
            className="w-full btn-iq-primary disabled:opacity-50"
          >
            {isLoading ? 'Signing in...' : 'Sign In'}
          </button>
        </form>

        <p className="text-sm text-iq-medium text-center mt-4">
          Don't have an account?{' '}
          <Link href="/auth/register" className="text-iq-blue font-semibold">
            Create one
          </Link>
        </p>
      </div>
    </div>
  )
}
