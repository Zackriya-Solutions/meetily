'use client'

import React from 'react'
import { ChevronLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'

interface AppHeaderProps {
  title?: string
  showBack?: boolean
  rightContent?: React.ReactNode
}

export default function AppHeader({ title, showBack = false, rightContent }: AppHeaderProps) {
  const router = useRouter()

  return (
    <div
      className="sticky top-0 z-40"
      style={{ background: 'linear-gradient(135deg, #2276aa, #1caac9)' }}
    >
      {/* Safe area spacer — fills the iOS status bar area with the gradient */}
      <div style={{ height: 'env(safe-area-inset-top, 0px)' }} />
      {/* Header content — only renders if there's a title, back button, or right content */}
      {(title || showBack || rightContent) && (
        <header className="text-white px-4 py-3 flex items-center gap-3">
          {showBack && (
            <button onClick={() => router.back()} className="p-1 -ml-1 rounded-lg hover:bg-white/10">
              <ChevronLeft className="w-5 h-5" />
            </button>
          )}
          {title && <h1 className="text-lg font-bold flex-1">{title}</h1>}
          {rightContent}
        </header>
      )}
    </div>
  )
}
