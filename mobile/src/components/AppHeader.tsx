'use client'

import React from 'react'
import { ChevronLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'
import { useHeaderContext } from '@/contexts/HeaderContext'

export default function AppHeader() {
  const router = useRouter()
  const { title, subtitle, showBack, rightContent } = useHeaderContext()

  return (
    <header
      className="sticky top-0 z-40 w-full flex-shrink-0 text-white"
      style={{
        background: 'linear-gradient(135deg, #2276aa, #1caac9)',
        paddingTop: 'calc(12px + env(safe-area-inset-top, 0px))',
        paddingBottom: '12px',
        paddingLeft: '16px',
        paddingRight: '16px',
      }}
    >
      <div className="flex items-center gap-3">
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
