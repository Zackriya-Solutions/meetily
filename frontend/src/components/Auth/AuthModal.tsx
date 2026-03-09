'use client'

import React, { useState, useEffect } from 'react'
import { LoginForm } from './LoginForm'
import { RegisterForm } from './RegisterForm'
import { ForgotPasswordForm } from './ForgotPasswordForm'
import { ResetPasswordForm } from './ResetPasswordForm'
import { VerifyEmailForm } from './VerifyEmailForm'

type AuthMode = 'login' | 'register' | 'forgot-password' | 'reset-password' | 'verify-email'

const TITLES: Record<AuthMode, string> = {
  'login': 'Sign In',
  'register': 'Create Account',
  'forgot-password': 'Forgot Password',
  'reset-password': 'Reset Password',
  'verify-email': 'Verify Email',
}

interface AuthModalProps {
  isOpen: boolean
  onClose: () => void
  onSuccess: () => void
  deviceId: string
}

export function AuthModal({ isOpen, onClose, onSuccess, deviceId }: AuthModalProps) {
  const [mode, setMode] = useState<AuthMode>('login')
  const [resetEmail, setResetEmail] = useState('')
  const [verifyEmail, setVerifyEmail] = useState('')

  // Reset to login when modal opens
  useEffect(() => {
    if (isOpen) {
      setMode('login')
      setResetEmail('')
      setVerifyEmail('')
    }
  }, [isOpen])

  if (!isOpen) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-white rounded-lg shadow-xl w-full max-w-sm mx-4 p-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-gray-900">
            {TITLES[mode]}
          </h2>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 text-xl leading-none"
          >
            &times;
          </button>
        </div>

        {mode === 'login' && (
          <LoginForm
            deviceId={deviceId}
            onSwitchToRegister={() => setMode('register')}
            onForgotPassword={() => setMode('forgot-password')}
            onNeedsVerification={(email) => {
              setVerifyEmail(email)
              setMode('verify-email')
            }}
            onSuccess={onSuccess}
          />
        )}

        {mode === 'register' && (
          <RegisterForm
            deviceId={deviceId}
            onSwitchToLogin={() => setMode('login')}
            onNeedsVerification={(email) => {
              setVerifyEmail(email)
              setMode('verify-email')
            }}
            onSuccess={onSuccess}
          />
        )}

        {mode === 'forgot-password' && (
          <ForgotPasswordForm
            onSwitchToLogin={() => setMode('login')}
            onCodeSent={(email) => {
              setResetEmail(email)
              setMode('reset-password')
            }}
          />
        )}

        {mode === 'reset-password' && (
          <ResetPasswordForm
            email={resetEmail}
            onSwitchToLogin={() => setMode('login')}
            onSuccess={() => setMode('login')}
          />
        )}

        {mode === 'verify-email' && (
          <VerifyEmailForm
            email={verifyEmail}
            onSwitchToLogin={() => setMode('login')}
            onSuccess={() => setMode('login')}
          />
        )}
      </div>
    </div>
  )
}
