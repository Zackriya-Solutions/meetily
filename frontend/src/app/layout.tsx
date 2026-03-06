'use client'

import './globals.css'
import { Source_Sans_3 } from 'next/font/google'
import { usePathname } from 'next/navigation'
import { Toaster } from 'sonner'
import "sonner/dist/styles.css"
import Sidebar from '@/components/Sidebar'
import { SidebarProvider } from '@/components/Sidebar/SidebarProvider'
import MainContent from '@/components/MainContent'
import { CalendarConnectPrompt } from '@/components/CalendarConnectPrompt'
import AnalyticsProvider from '@/components/AnalyticsProvider'
import { AuthProvider } from '@/components/AuthProvider'
import { TooltipProvider } from '@/components/ui/tooltip'
import { RecordingStateProvider } from '@/contexts/RecordingStateContext'
// import { OllamaDownloadProvider } from '@/contexts/OllamaDownloadContext'

const sourceSans3 = Source_Sans_3({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-source-sans-3',
})

// export { metadata } from './metadata'

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const pathname = usePathname();
  const isAuthPage = pathname === '/login';

  return (
    <html lang="en">
      <body className={`${sourceSans3.variable} font-sans`}>
        <AuthProvider>
          <AnalyticsProvider>
            <RecordingStateProvider>
              <SidebarProvider>
                {/* <OllamaDownloadProvider> */}
                <TooltipProvider>
                  {/* <div className="titlebar h-8 w-full fixed top-0 left-0 bg-transparent" /> */}
                  {isAuthPage ? (
                    <div className="min-h-screen">
                      {children}
                    </div>
                  ) : (
                    <div className="flex">
                      <Sidebar />
                      <MainContent>
                        <CalendarConnectPrompt />
                        {children}
                      </MainContent>
                    </div>
                  )}
                </TooltipProvider>
                {/* </OllamaDownloadProvider> */}
              </SidebarProvider>
            </RecordingStateProvider>
          </AnalyticsProvider>
        </AuthProvider>
        <Toaster position="bottom-center" richColors closeButton />
      </body>
    </html>
  )
}
