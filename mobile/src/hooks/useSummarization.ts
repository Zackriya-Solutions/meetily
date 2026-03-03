'use client'

import { useState, useEffect, useCallback, useRef } from 'react'
import { SummaryStatus } from '@/types'
import { startSummarization, pollSummaryStatus } from '@/services/summarizationService'
import { meetingRepository } from '@/services/meetingRepository'

const POLL_INTERVAL_MS = 3000

/**
 * Hook that manages summary generation and polls status until complete.
 */
export function useSummarization(meetingId: string | null) {
  const [status, setStatus] = useState<SummaryStatus | null>(null)
  const [isPolling, setIsPolling] = useState(false)
  const [isStarting, setIsStarting] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const stopPolling = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current)
      intervalRef.current = null
    }
    setIsPolling(false)
  }, [])

  const startPolling = useCallback(() => {
    if (!meetingId || intervalRef.current) return

    setIsPolling(true)

    const poll = async () => {
      try {
        const result = await pollSummaryStatus(meetingId)
        setStatus(result)

        if (result.status === 'completed') {
          stopPolling()
          // Update local meeting with summary
          if (result.summary) {
            await meetingRepository.updateMeeting(meetingId, {
              summary: result.summary,
              status: 'completed',
            })
          }
        } else if (result.status === 'failed') {
          stopPolling()
          await meetingRepository.updateMeeting(meetingId, { status: 'completed' })
        }
      } catch (e) {
        console.warn('[useSummarization] Poll error:', e)
      }
    }

    poll()
    intervalRef.current = setInterval(poll, POLL_INTERVAL_MS)
  }, [meetingId, stopPolling])

  const generate = useCallback(
    async (options?: { provider?: string; model?: string; customPrompt?: string }) => {
      if (!meetingId || isStarting) return

      setIsStarting(true)
      try {
        await meetingRepository.updateMeeting(meetingId, { status: 'summarizing' })
        await startSummarization(meetingId, options)
        startPolling()
      } catch (e) {
        console.warn('[useSummarization] Start error:', e)
        await meetingRepository.updateMeeting(meetingId, { status: 'completed' })
        throw e
      } finally {
        setIsStarting(false)
      }
    },
    [meetingId, isStarting, startPolling],
  )

  // Cleanup on unmount
  useEffect(() => {
    return () => stopPolling()
  }, [stopPolling])

  return { status, isPolling, isStarting, generate }
}
