'use client'

import './globals.css'
import { Poppins } from 'next/font/google'
import { useEffect, useState } from 'react'
import { useRouter } from 'next/navigation'
import { AuthProvider, useAuth } from '@/contexts/AuthContext'
import { SyncProvider } from '@/contexts/SyncContext'
import { RecordingProvider } from '@/contexts/RecordingContext'
import { initUsageService } from '@/services/usageService'
import { checkBiometricOnResume } from '@/services/biometricAuth'
import { initNotifications } from '@/services/pushNotifications'
import { registerDeepLinkHandler, parseDeepLink } from '@/services/deepLinking'
import { Toaster } from 'sonner'
import TabBar from '@/components/TabBar'
import AppHeader from '@/components/AppHeader'
import AuthPrompt from '@/components/AuthPrompt'
import SplashScreen from '@/components/SplashScreen'

const poppins = Poppins({
  subsets: ['latin'],
  weight: ['300', '400', '500', '600', '700'],
  variable: '--font-poppins',
})

function AuthGatedApp({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth()
  const [authScreen, setAuthScreen] = useState<'prompt' | 'login' | 'register' | 'forgot' | 'verify' | 'reset'>('prompt')
  const [authEmail, setAuthEmail] = useState('')

  if (isLoading) {
    return <SplashScreen />
  }

  // Not authenticated — show inline auth flow (no page navigation)
  if (!isAuthenticated) {
    return (
      <main className="flex flex-col h-screen bg-iq-light">
        <AppHeader title="IQ:capture" />
        <div className="flex-1 overflow-y-auto">
          {authScreen === 'prompt' && (
            <AuthPrompt onNavigate={(page) => setAuthScreen(page)} />
          )}
          {authScreen === 'login' && (
            <InlineLoginForm
              onForgot={() => setAuthScreen('forgot')}
              onRegister={() => setAuthScreen('register')}
              onNeedsVerification={(email) => { setAuthEmail(email); setAuthScreen('verify') }}
            />
          )}
          {authScreen === 'register' && (
            <InlineRegisterForm
              onLogin={() => setAuthScreen('login')}
              onNeedsVerification={(email) => { setAuthEmail(email); setAuthScreen('verify') }}
            />
          )}
          {authScreen === 'forgot' && (
            <InlineForgotForm
              onBack={() => setAuthScreen('login')}
              onCodeSent={(email) => { setAuthEmail(email); setAuthScreen('reset') }}
            />
          )}
          {authScreen === 'verify' && (
            <InlineVerifyForm
              email={authEmail}
              onVerified={() => setAuthScreen('login')}
              onBack={() => setAuthScreen('login')}
            />
          )}
          {authScreen === 'reset' && (
            <InlineResetForm
              email={authEmail}
              onReset={() => setAuthScreen('login')}
              onBack={() => setAuthScreen('forgot')}
            />
          )}
        </div>
      </main>
    )
  }

  return (
    <SyncProvider>
      <RecordingProvider>
        <main className="flex flex-col h-screen w-full overflow-x-hidden">
          <AppHeader />
          <div className="flex-1 overflow-y-auto overflow-x-hidden pb-16 px-4">
            {children}
          </div>
          <TabBar />
        </main>
      </RecordingProvider>
    </SyncProvider>
  )
}

// ── Inline Auth Forms (no page navigation) ──────────────────────────

function InlineLoginForm({ onForgot, onRegister, onNeedsVerification }: {
  onForgot: () => void
  onRegister: () => void
  onNeedsVerification: (email: string) => void
}) {
  const { login, error, clearError, isLoading } = useAuth()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    clearError()
    const result = await login(email, password)
    if (result === 'EMAIL_NOT_VERIFIED') {
      onNeedsVerification(email)
    }
  }

  

  return (
    <div className="flex flex-col items-center px-6 pt-12 pb-8 bg-iq-light min-h-full overflow-y-auto">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
        <p className="text-sm text-iq-medium mb-6">Welcome back</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Email</label>
            <input type="email" value={email} onChange={e => setEmail(e.target.value)} required autoComplete="email" className="input-iq" placeholder="you@example.com" />
          </div>
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="detail-label">Password</label>
              <button type="button" onClick={onForgot} className="text-xs text-iq-blue font-semibold">Forgot password?</button>
            </div>
            <input type="password" value={password} onChange={e => setPassword(e.target.value)} required autoComplete="current-password" className="input-iq" placeholder="Your password" />
          </div>
          {error && <div className="status-banner status-banner-danger"><span className="text-sm text-iq-red">{error}</span></div>}
          <button type="submit" disabled={isLoading} className="w-full btn-iq-primary disabled:opacity-50">{isLoading ? 'Signing in...' : 'Sign In'}</button>
        </form>
        <p className="text-sm text-iq-medium text-center mt-4">
          Don't have an account? <button onClick={onRegister} className="text-iq-blue font-semibold">Create one</button>
        </p>
      </div>
    </div>
  )
}

function InlineRegisterForm({ onLogin, onNeedsVerification }: {
  onLogin: () => void
  onNeedsVerification: (email: string) => void
}) {
  const { register, error, clearError, isLoading } = useAuth()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [inviteCode, setInviteCode] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    clearError()
    const success = await register(email, password, displayName || undefined, inviteCode)
    if (success) onNeedsVerification(email)
  }

  

  return (
    <div className="flex flex-col items-center px-6 pt-12 pb-8 bg-iq-light min-h-full overflow-y-auto">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
        <p className="text-sm text-iq-medium mb-6">Create your account</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Invite Code</label>
            <input type="text" value={inviteCode} onChange={e => setInviteCode(e.target.value)} required className="input-iq font-mono" placeholder="Paste your invite code" />
            <p className="text-xs text-iq-medium mt-1">Your organisation admin will provide this</p>
          </div>
          <div>
            <label className="detail-label mb-1 block">Name <span className="normal-case font-normal">(optional)</span></label>
            <input type="text" value={displayName} onChange={e => setDisplayName(e.target.value)} autoComplete="name" className="input-iq" placeholder="Your name" />
          </div>
          <div>
            <label className="detail-label mb-1 block">Email</label>
            <input type="email" value={email} onChange={e => setEmail(e.target.value)} required autoComplete="email" className="input-iq" placeholder="you@example.com" />
          </div>
          <div>
            <label className="detail-label mb-1 block">Password</label>
            <input type="password" value={password} onChange={e => setPassword(e.target.value)} required minLength={8} autoComplete="new-password" className="input-iq" placeholder="Min 8 characters" />
            <p className="text-xs text-iq-medium mt-1">Must include uppercase, lowercase, and a number</p>
          </div>
          {error && <div className="status-banner status-banner-danger"><span className="text-sm text-iq-red">{error}</span></div>}
          <button type="submit" disabled={isLoading} className="w-full btn-iq-primary disabled:opacity-50">{isLoading ? 'Creating account...' : 'Create Account'}</button>
        </form>
        <p className="text-sm text-iq-medium text-center mt-4">
          Already have an account? <button onClick={onLogin} className="text-iq-blue font-semibold">Sign in</button>
        </p>
      </div>
    </div>
  )
}

function InlineForgotForm({ onBack, onCodeSent }: { onBack: () => void; onCodeSent: (email: string) => void }) {
  const [email, setEmail] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setError('')
    try {
      const { forgotPassword } = await import('@/services/authService')
      await forgotPassword(email)
      onCodeSent(email)
    } catch (err: any) {
      setError(err.message || 'Request failed')
    } finally {
      setLoading(false)
    }
  }

  

  return (
    <div className="flex flex-col items-center px-6 pt-12 pb-8 bg-iq-light min-h-full overflow-y-auto">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
        <p className="text-sm text-iq-medium mb-6">Reset your password</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Email</label>
            <input type="email" value={email} onChange={e => setEmail(e.target.value)} required className="input-iq" placeholder="you@example.com" />
          </div>
          {error && <div className="status-banner status-banner-danger"><span className="text-sm text-iq-red">{error}</span></div>}
          <button type="submit" disabled={loading} className="w-full btn-iq-primary disabled:opacity-50">{loading ? 'Sending...' : 'Send Reset Code'}</button>
        </form>
        <p className="text-sm text-iq-medium text-center mt-4">
          <button onClick={onBack} className="text-iq-blue font-semibold">Back to sign in</button>
        </p>
      </div>
    </div>
  )
}

function InlineVerifyForm({ email, onVerified, onBack }: { email: string; onVerified: () => void; onBack: () => void }) {
  const [code, setCode] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [success, setSuccess] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setError('')
    try {
      const { verifyEmail } = await import('@/services/authService')
      await verifyEmail(email, code)
      setSuccess(true)
      setTimeout(onVerified, 1500)
    } catch (err: any) {
      setError(err.message || 'Verification failed')
    } finally {
      setLoading(false)
    }
  }

  

  return (
    <div className="flex flex-col items-center px-6 pt-12 pb-8 bg-iq-light min-h-full overflow-y-auto">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
        <p className="text-sm text-iq-medium mb-6">Enter the 6-digit code sent to {email}</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Verification Code</label>
            <input type="text" value={code} onChange={e => setCode(e.target.value)} required maxLength={6} className="input-iq font-mono text-center tracking-widest" placeholder="000000" />
          </div>
          {success && <div className="status-banner status-banner-success"><span className="text-sm text-iq-green">Email verified!</span></div>}
          {error && <div className="status-banner status-banner-danger"><span className="text-sm text-iq-red">{error}</span></div>}
          <button type="submit" disabled={loading || success} className="w-full btn-iq-primary disabled:opacity-50">{loading ? 'Verifying...' : 'Verify'}</button>
        </form>
        <p className="text-sm text-iq-medium text-center mt-4">
          <button onClick={onBack} className="text-iq-blue font-semibold">Back to sign in</button>
        </p>
      </div>
    </div>
  )
}

function InlineResetForm({ email, onReset, onBack }: { email: string; onReset: () => void; onBack: () => void }) {
  const [code, setCode] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setError('')
    try {
      const { resetPassword } = await import('@/services/authService')
      await resetPassword(email, code, newPassword)
      onReset()
    } catch (err: any) {
      setError(err.message || 'Reset failed')
    } finally {
      setLoading(false)
    }
  }

  

  return (
    <div className="flex flex-col items-center px-6 pt-12 pb-8 bg-iq-light min-h-full overflow-y-auto">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold text-iq-dark mb-1">IQ:capture</h1>
        <p className="text-sm text-iq-medium mb-6">Enter the code sent to {email}</p>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="detail-label mb-1 block">Reset Code</label>
            <input type="text" value={code} onChange={e => setCode(e.target.value)} required maxLength={6} className="input-iq font-mono text-center tracking-widest" placeholder="000000" />
          </div>
          <div>
            <label className="detail-label mb-1 block">New Password</label>
            <input type="password" value={newPassword} onChange={e => setNewPassword(e.target.value)} required minLength={8} className="input-iq" placeholder="Min 8 characters" />
          </div>
          {error && <div className="status-banner status-banner-danger"><span className="text-sm text-iq-red">{error}</span></div>}
          <button type="submit" disabled={loading} className="w-full btn-iq-primary disabled:opacity-50">{loading ? 'Resetting...' : 'Reset Password'}</button>
        </form>
        <p className="text-sm text-iq-medium text-center mt-4">
          <button onClick={onBack} className="text-iq-blue font-semibold">Back</button>
        </p>
      </div>
    </div>
  )
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const router = useRouter()

  useEffect(() => {
    initUsageService()
    initNotifications()

    // Deep linking
    const unregister = registerDeepLinkHandler((path, params) => {
      const route = parseDeepLink(path, params)
      if (route) router.push(route)
    })

    // Biometric lock on app resume
    const handleVisibilityChange = async () => {
      if (document.visibilityState === 'visible') {
        const passed = await checkBiometricOnResume()
        if (!passed) {
          // User failed biometric — could show a lock screen
          // For now, the biometric prompt will re-appear on next resume
        }
      }
    }
    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
      unregister()
    }
  }, [router])

  return (
    <html lang="en">
      <head>
        <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, user-scalable=no" />
        <meta name="theme-color" content="#2276aa" />
        <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
        <meta name="apple-mobile-web-app-capable" content="yes" />
        <meta name="apple-mobile-web-app-title" content="IQ:capture" />
        <link rel="icon" href="/favicon.png" type="image/png" />
        <link rel="apple-touch-icon" href="/icon.png" />
        <link rel="manifest" href="/manifest.json" />
      </head>
      <body className={`${poppins.variable} font-sans antialiased bg-iq-light text-iq-dark`}>
        <AuthProvider>
          <AuthGatedApp>{children}</AuthGatedApp>
        </AuthProvider>
        <Toaster position="top-center" richColors closeButton />
      </body>
    </html>
  )
}
