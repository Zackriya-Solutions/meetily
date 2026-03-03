'use client'

import React from 'react'
import Link from 'next/link'

export default function AuthPrompt() {
  return (
    <div className="flex flex-col items-center justify-center h-full px-6 text-center">
      <div className="w-16 h-16 bg-blue-100 rounded-full flex items-center justify-center mb-4">
        <svg className="w-8 h-8 text-blue-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
        </svg>
      </div>
      <h2 className="text-xl font-semibold text-gray-900 mb-2">Sign in to get started</h2>
      <p className="text-sm text-gray-500 mb-6">
        Create an account or sign in to record and transcribe your meetings.
      </p>
      <div className="flex gap-3">
        <Link
          href="/auth/login"
          className="px-6 py-2.5 bg-blue-600 text-white rounded-lg text-sm font-medium"
        >
          Sign In
        </Link>
        <Link
          href="/auth/register"
          className="px-6 py-2.5 border border-gray-300 text-gray-700 rounded-lg text-sm font-medium"
        >
          Create Account
        </Link>
      </div>
    </div>
  )
}
