'use client'

import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react'
import { syncService } from '@/services/syncService'
import { getDatabase, initializeDatabase } from '@/services/database'
import { useAuth } from './AuthContext'

interface SyncContextValue {
  isOnline: boolean
  isSyncing: boolean
  pendingCount: number
  lastSyncedAt: string | null
  forceSync: () => Promise<void>
}

const SyncContext = createContext<SyncContextValue | null>(null)

export function useSync(): SyncContextValue {
  const ctx = useContext(SyncContext)
  if (!ctx) throw new Error('useSync must be used within SyncProvider')
  return ctx
}

export function SyncProvider({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuth()
  const [isOnline, setIsOnline] = useState(true)
  const [isSyncing, setIsSyncing] = useState(false)
  const [pendingCount, setPendingCount] = useState(0)
  const [lastSyncedAt, setLastSyncedAt] = useState<string | null>(null)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // Initialize database and sync service
  useEffect(() => {
    initializeDatabase().catch(console.warn)
  }, [])

  // Network status
  useEffect(() => {
    const handleOnline = () => setIsOnline(true)
    const handleOffline = () => setIsOnline(false)
    setIsOnline(navigator.onLine)
    window.addEventListener('online', handleOnline)
    window.addEventListener('offline', handleOffline)
    return () => {
      window.removeEventListener('online', handleOnline)
      window.removeEventListener('offline', handleOffline)
    }
  }, [])

  // Start/stop sync service based on auth status
  useEffect(() => {
    if (isAuthenticated) {
      syncService.start()
    } else {
      syncService.stop()
    }
    return () => syncService.stop()
  }, [isAuthenticated])

  // Poll pending count periodically
  useEffect(() => {
    const updatePending = async () => {
      try {
        const db = getDatabase()
        const count = await db.getPendingCount()
        setPendingCount(count)
        const last = await db.getSyncState('last_sync_at')
        setLastSyncedAt(last)
      } catch {
        // ignore
      }
    }

    updatePending()
    pollRef.current = setInterval(updatePending, 5000)
    return () => {
      if (pollRef.current) clearInterval(pollRef.current)
    }
  }, [])

  const forceSync = useCallback(async () => {
    if (!isOnline || isSyncing) return
    setIsSyncing(true)
    try {
      await syncService.sync()
      const db = getDatabase()
      const count = await db.getPendingCount()
      setPendingCount(count)
      const last = await db.getSyncState('last_sync_at')
      setLastSyncedAt(last)
    } finally {
      setIsSyncing(false)
    }
  }, [isOnline, isSyncing])

  return (
    <SyncContext.Provider
      value={{ isOnline, isSyncing, pendingCount, lastSyncedAt, forceSync }}
    >
      {children}
    </SyncContext.Provider>
  )
}
