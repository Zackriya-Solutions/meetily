'use client'

import './globals.css'
import { Source_Sans_3 } from 'next/font/google'
import { useEffect } from 'react'
import { AuthProvider } from '@/contexts/AuthContext'
import { SyncProvider } from '@/contexts/SyncContext'
import { RecordingProvider } from '@/contexts/RecordingContext'
import { initUsageService } from '@/services/usageService'
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
  useEffect(() => {
    initUsageService()
  }, [])

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
