'use client'

import { useState, useEffect, useCallback } from 'react'
import { getTranscriptionQuota } from '@/services/transcriptionService'
import { useAuth } from '@/contexts/AuthContext'

interface QuotaInfo {
  remaining_minutes: number
  plan_limit: number
  used_minutes: number
}

/**
 * Hook that fetches and caches the user's transcription quota.
 * Refreshes on mount and provides a manual refresh function.
 */
export function useQuota() {
  const { isAuthenticated } = useAuth()
  const [quota, setQuota] = useState<QuotaInfo | null>(null)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    if (!isAuthenticated) return
    setLoading(true)
    try {
      const q = await getTranscriptionQuota()
      setQuota(q)
    } catch {
      // Fail silently — quota check is non-critical
    } finally {
      setLoading(false)
    }
  }, [isAuthenticated])

  useEffect(() => {
    refresh()
  }, [refresh])

  const hasQuota = quota ? quota.remaining_minutes > 0 : true // Default allow if unknown

  return { quota, loading, refresh, hasQuota }
}
