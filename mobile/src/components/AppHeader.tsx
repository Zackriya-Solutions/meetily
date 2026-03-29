'use client'

import React from 'react'
import { ChevronLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'

interface AppHeaderProps {
  title?: string
  showBack?: boolean
  rightContent?: React.ReactNode
}

export default function AppHeader({ title = 'IQ:capture', showBack = false, rightContent }: AppHeaderProps) {
  const router = useRouter()

  return (
    <header
      className="sticky top-0 z-40 text-white px-4 pb-3 flex items-center gap-3"
      style={{
        background: 'linear-gradient(135deg, #2276aa, #1caac9)',
        paddingTop: 'calc(env(safe-area-inset-top, 0px) + 12px)',
      }}
    >
      {showBack && (
        <button onClick={() => router.back()} className="p-1 -ml-1 rounded-lg hover:bg-white/10">
          <ChevronLeft className="w-5 h-5" />
        </button>
      )}
      <h1 className="text-lg font-bold flex-1">{title}</h1>
      {rightContent}
    </header>
  )
}
