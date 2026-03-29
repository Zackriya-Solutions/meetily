'use client'

import React from 'react'

export default function AuthPrompt() {
  const navigate = (path: string) => {
    window.location.href = path
  }

  return (
    <div className="flex flex-col items-center justify-center h-full px-6 text-center">
      <div className="w-16 h-16 bg-iq-blue/10 rounded-full flex items-center justify-center mb-4">
        <svg className="w-8 h-8 text-iq-blue" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
        </svg>
      </div>
      <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
      <p className="text-sm text-iq-medium mb-6">
        Sign in to record and transcribe your meetings.
      </p>
      <div className="flex gap-3">
        <a
          href="/auth/login"
          className="btn-iq-primary no-underline"
        >
          Sign In
        </a>
        <a
          href="/auth/register"
          className="btn-iq-outline no-underline"
        >
          Create Account
        </a>
      </div>
    </div>
  )
}
