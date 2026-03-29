'use client'

import React, { useState } from 'react'
import { useRouter } from 'next/navigation'
import { useAuth } from '@/contexts/AuthContext'
import Link from 'next/link'

export default function RegisterPage() {
  const { register, error, clearError, isLoading } = useAuth()
  const router = useRouter()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [inviteCode, setInviteCode] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    clearError()
    const success = await register(email, password, displayName || undefined, inviteCode)
    if (success) {
      router.replace(`/auth/verify-email?email=${encodeURIComponent(email)}`)
    }
  }

  const inputClass = "w-full px-3 py-2.5 border border-iq-light-shade rounded-iq-lg text-sm text-iq-dark bg-white focus:outline-none focus:ring-2 focus:ring-iq-blue focus:border-transparent"

  return (
    <div className="flex flex-col items-center justify-center min-h-screen px-6 bg-iq-light">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">Create account</h1>
        <p className="text-sm text-iq-medium mb-6">Get started with IQ:capture</p>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Invite Code</label>
            <input
              type="text"
              value={inviteCode}
              onChange={(e) => setInviteCode(e.target.value)}
              required
              className={`${inputClass} font-mono`}
              placeholder="Paste your invite code"
            />
            <p className="text-xs text-iq-medium mt-1">Your organisation admin will provide this</p>
          </div>

          <div>
            <label className="detail-label mb-1 block">Name <span className="normal-case font-normal">(optional)</span></label>
            <input
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              autoComplete="name"
              className={inputClass}
              placeholder="Your name"
            />
          </div>

          <div>
            <label className="detail-label mb-1 block">Email</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              autoComplete="email"
              className={inputClass}
              placeholder="you@example.com"
            />
          </div>

          <div>
            <label className="detail-label mb-1 block">Password</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={8}
              autoComplete="new-password"
              className={inputClass}
              placeholder="Min 8 characters"
            />
            <p className="text-xs text-iq-medium mt-1">
              Must include uppercase, lowercase, and a number
            </p>
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
            {isLoading ? 'Creating account...' : 'Create Account'}
          </button>
        </form>

        <p className="text-sm text-iq-medium text-center mt-4">
          Already have an account?{' '}
          <Link href="/auth/login" className="text-iq-blue font-semibold">
            Sign in
          </Link>
        </p>
      </div>
    </div>
  )
}
