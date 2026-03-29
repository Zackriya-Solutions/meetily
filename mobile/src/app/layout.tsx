'use client'

import './globals.css'
import { Poppins } from 'next/font/google'
import { useEffect } from 'react'
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
import AuthPrompt from '@/components/AuthPrompt'

const poppins = Poppins({
  subsets: ['latin'],
  weight: ['300', '400', '500', '600', '700'],
  variable: '--font-poppins',
})

function AuthGatedApp({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth()
  const router = useRouter()

  // Auth pages (login, register, etc.) are not gated
  const isAuthPage = typeof window !== 'undefined' && window.location.pathname.startsWith('/auth')

  if (isLoading) {
    return (
      <main className="flex flex-col items-center justify-center h-screen bg-iq-light">
        <div className="w-8 h-8 border-4 border-iq-blue border-t-transparent rounded-full animate-spin" />
        <p className="text-sm text-iq-medium mt-4">Loading...</p>
      </main>
    )
  }

  if (!isAuthenticated && !isAuthPage) {
    return (
      <main className="flex flex-col h-screen">
        <div className="flex-1">
          <AuthPrompt />
        </div>
      </main>
    )
  }

  return (
    <SyncProvider>
      <RecordingProvider>
        <main className="flex flex-col h-screen">
          <div className="flex-1 overflow-y-auto pb-16">
            {children}
          </div>
          {isAuthenticated && <TabBar />}
        </main>
      </RecordingProvider>
    </SyncProvider>
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
        <meta
          name="viewport"
          content="width=device-width, initial-scale=1, viewport-fit=cover, user-scalable=no"
        />
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
