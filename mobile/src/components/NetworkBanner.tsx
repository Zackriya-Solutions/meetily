'use client'

import React from 'react'
import { useSync } from '@/contexts/SyncContext'
import { WifiOff } from 'lucide-react'

export default function NetworkBanner() {
  const { isOnline, pendingCount } = useSync()

  if (isOnline && pendingCount === 0) return null

  return (
    <div className={`px-4 py-2 text-xs font-medium flex items-center gap-2 ${
      isOnline ? 'bg-yellow-50 text-yellow-700' : 'bg-red-50 text-red-700'
    }`}>
      {!isOnline && (
        <>
          <WifiOff className="w-3.5 h-3.5" />
          <span>You're offline. Changes will sync when you reconnect.</span>
        </>
      )}
      {isOnline && pendingCount > 0 && (
        <span>Syncing {pendingCount} pending item{pendingCount !== 1 ? 's' : ''}...</span>
      )}
    </div>
  )
}
