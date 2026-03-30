'use client'

import React, { useEffect, useState, useCallback } from 'react'
import Link from 'next/link'
import { format } from 'date-fns'
import { meetingRepository } from '@/services/meetingRepository'
import { useSync } from '@/contexts/SyncContext'
import { useHeader } from '@/contexts/HeaderContext'
import { Meeting } from '@/types'
import MeetingCard from './MeetingCard'
import NetworkBanner from './NetworkBanner'

export default function MeetingsList() {
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const [loading, setLoading] = useState(true)
  const { forceSync, isSyncing } = useSync()

  const loadMeetings = useCallback(async () => {
    try {
      const data = await meetingRepository.getMeetings()
      setMeetings(data)
    } catch (e) {
      console.warn('[MeetingsList] Failed to load meetings:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  useHeader({
    title: 'Meetings',
    rightContent: (
      <button
        onClick={async () => { await forceSync(); await loadMeetings() }}
        disabled={isSyncing}
        className="text-sm text-white/80 font-medium disabled:opacity-50"
      >
        {isSyncing ? 'Syncing...' : 'Refresh'}
      </button>
    ),
  })

  useEffect(() => {
    loadMeetings()
  }, [loadMeetings])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-iq-blue" />
      </div>
    )
  }

  return (
    <div className="flex flex-col">
      <NetworkBanner />

      {/* Meeting list */}
      {meetings.length === 0 ? (
        <div className="flex flex-col items-center justify-center px-6 py-20 text-center">
          <p className="text-iq-medium mb-2">No meetings yet</p>
          <p className="text-sm text-iq-medium">
            Tap the Record tab to start your first meeting.
          </p>
        </div>
      ) : (
        <div className="space-y-2 pb-4">
          {meetings.map((meeting) => (
            <Link key={meeting.meeting_id} href={`/meeting?id=${meeting.meeting_id}`}>
              <MeetingCard meeting={meeting} />
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
