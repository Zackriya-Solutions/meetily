'use client'

import React, { useEffect, useState, useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { Meeting } from '@/types'
import { meetingRepository } from '@/services/meetingRepository'
import { useSummarization } from '@/hooks/useSummarization'
import { useTranscription } from '@/hooks/useTranscription'
import TranscriptView from './TranscriptView'
import SummaryView from './SummaryView'
import { ArrowLeft, FileText, BookOpen } from 'lucide-react'

interface MeetingDetailProps {
  meetingId: string
}

export default function MeetingDetail({ meetingId }: MeetingDetailProps) {
  const router = useRouter()
  const [meeting, setMeeting] = useState<Meeting | null>(null)
  const [activeTab, setActiveTab] = useState<'transcript' | 'summary'>('transcript')
  const [loading, setLoading] = useState(true)

  const { generate: generateSummary, isPolling: isSummaryPolling, isStarting } = useSummarization(meetingId)

  // Reload meeting data from local DB (picks up changes from hooks)
  const refreshMeeting = useCallback(async () => {
    try {
      const data = await meetingRepository.getMeeting(meetingId)
      setMeeting(data)
    } catch (e) {
      console.warn('[MeetingDetail] Failed to refresh meeting:', e)
    }
  }, [meetingId])

  useEffect(() => {
    const load = async () => {
      try {
        const data = await meetingRepository.getMeeting(meetingId)
        setMeeting(data)
      } catch (e) {
        console.warn('[MeetingDetail] Failed to load meeting:', e)
      } finally {
        setLoading(false)
      }
    }
    load()
  }, [meetingId])

  // Periodically refresh meeting data while summarization is polling
  useEffect(() => {
    if (!isSummaryPolling) return
    const interval = setInterval(refreshMeeting, 3000)
    return () => clearInterval(interval)
  }, [isSummaryPolling, refreshMeeting])

  const handleGenerateSummary = useCallback(async () => {
    try {
      await generateSummary()
      await refreshMeeting()
    } catch (e) {
      console.warn('[MeetingDetail] Summary generation failed:', e)
    }
  }, [generateSummary, refreshMeeting])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-iq-blue" />
      </div>
    )
  }

  if (!meeting) {
    return (
      <div className="flex flex-col items-center justify-center h-full px-6 text-center">
        <p className="text-iq-medium">Meeting not found</p>
        <button onClick={() => router.back()} className="text-iq-blue text-sm mt-2">
          Go back
        </button>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-4 pt-4 pb-2 border-b border-iq-light-shade">
        <div className="flex items-center gap-3 mb-2">
          <button onClick={() => router.back()} className="p-1 -ml-1">
            <ArrowLeft className="w-5 h-5 text-iq-medium" />
          </button>
          <h1 className="text-lg font-semibold text-iq-dark truncate flex-1">
            {meeting.title || 'Untitled Meeting'}
          </h1>
        </div>

        {/* Status indicator for pending operations */}
        {meeting.status !== 'completed' && (
          <div className="text-xs text-iq-blue mb-2">
            {meeting.status === 'pending_upload' && 'Waiting to upload audio...'}
            {meeting.status === 'uploading' && 'Uploading audio...'}
            {meeting.status === 'transcribing' && 'Transcription in progress...'}
            {meeting.status === 'summarizing' && 'Generating summary...'}
          </div>
        )}

        {/* Tabs */}
        <div className="flex gap-4">
          <button
            onClick={() => setActiveTab('transcript')}
            className={`flex items-center gap-1.5 pb-2 text-sm font-medium border-b-2 ${
              activeTab === 'transcript'
                ? 'border-iq-blue text-iq-blue'
                : 'border-transparent text-iq-medium'
            }`}
          >
            <FileText className="w-4 h-4" />
            Transcript
          </button>
          <button
            onClick={() => setActiveTab('summary')}
            className={`flex items-center gap-1.5 pb-2 text-sm font-medium border-b-2 ${
              activeTab === 'summary'
                ? 'border-iq-blue text-iq-blue'
                : 'border-transparent text-iq-medium'
            }`}
          >
            <BookOpen className="w-4 h-4" />
            Summary
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {activeTab === 'transcript' ? (
          <TranscriptView
            text={meeting.transcript_text}
            segments={meeting.transcript_segments}
          />
        ) : (
          <SummaryView
            summary={meeting.summary}
            meetingId={meetingId}
            status={meeting.status}
            onGenerateSummary={handleGenerateSummary}
            isGenerating={isStarting || isSummaryPolling}
          />
        )}
      </div>
    </div>
  )
}
