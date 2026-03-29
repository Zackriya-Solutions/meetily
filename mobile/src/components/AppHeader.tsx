'use client'

import React from 'react'
import { ChevronLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'

interface AppHeaderProps {
  title?: string
  subtitle?: string
  showBack?: boolean
  rightContent?: React.ReactNode
}

export default function AppHeader({
  title = 'IQ:capture',
  subtitle,
  showBack = false,
  rightContent,
}: AppHeaderProps) {
  const router = useRouter()

  return (
    <header
      className="sticky top-0 z-40 w-full flex-shrink-0"
      style={{ background: 'linear-gradient(135deg, #2276aa, #1caac9)' }}
    >
      {/* Status bar spacer */}
      <div style={{ height: 'env(safe-area-inset-top, 0px)' }} />
      {/* Header content */}
      <div className="flex items-center gap-3 px-4 py-3 text-white">
        {showBack && (
          <button onClick={() => router.back()} className="p-1 -ml-1 rounded-lg active:bg-white/10">
            <ChevronLeft className="w-5 h-5" />
          </button>
        )}
        <div className="flex-1 min-w-0">
          <h1 className="text-lg font-bold leading-tight truncate">{title}</h1>
          {subtitle && (
            <p className="text-xs text-white/60 truncate">{subtitle}</p>
          )}
        </div>
        {rightContent}
      </div>
    </header>
  )
}
