'use client'

import './globals.css'
import { Source_Sans_3 } from 'next/font/google'
import { useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { AuthProvider } from '@/contexts/AuthContext'
import { SyncProvider } from '@/contexts/SyncContext'
import { RecordingProvider } from '@/contexts/RecordingContext'
import { initUsageService } from '@/services/usageService'
import { checkBiometricOnResume } from '@/services/biometricAuth'
import { initNotifications } from '@/services/pushNotifications'
import { registerDeepLinkHandler, parseDeepLink } from '@/services/deepLinking'
import { Toaster } from 'sonner'
import TabBar from '@/components/TabBar'

const sourceSans3 = Source_Sans_3({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-source-sans-3',
})

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
      <body className={`${sourceSans3.variable} font-sans antialiased bg-white`}>
        <AuthProvider>
          <SyncProvider>
            <RecordingProvider>
              <main className="flex flex-col h-screen">
                <div className="flex-1 overflow-y-auto pb-16">
                  {children}
                </div>
                <TabBar />
              </main>
            </RecordingProvider>
          </SyncProvider>
        </AuthProvider>
        <Toaster position="top-center" richColors closeButton />
      </body>
    </html>
  )
}
