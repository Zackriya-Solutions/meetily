'use client'

import React, { useState, useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { useRecording } from '@/contexts/RecordingContext'
import { useQuota } from '@/hooks/useQuota'
import { Mic, Square, Pause, Play, AlertCircle } from 'lucide-react'

export default function RecordingScreen() {
  const router = useRouter()
  const { isRecording, isPaused, duration, startRecording, stopRecording, pauseRecording, resumeRecording } = useRecording()
  const { quota, hasQuota } = useQuota()
  const [meetingTitle, setMeetingTitle] = useState('')
  const [error, setError] = useState<string | null>(null)

  const formatDuration = (seconds: number) => {
    const h = Math.floor(seconds / 3600)
    const m = Math.floor((seconds % 3600) / 60)
    const s = Math.floor(seconds % 60)
    return h > 0
      ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
      : `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }

  const handleStart = useCallback(async () => {
    setError(null)
    try {
      await startRecording(meetingTitle || undefined)
    } catch (e: any) {
      if (e.message?.includes('permission') || e.message?.includes('Permission')) {
        setError('Microphone access is required. Please enable it in your device settings.')
      } else {
        setError(e.message || 'Failed to start recording')
      }
    }
  }, [startRecording, meetingTitle])

  const handleStop = useCallback(async () => {
    const meetingId = await stopRecording()
    if (meetingId) {
      router.push(`/meeting/${meetingId}`)
    }
  }, [stopRecording, router])

  return (
    <div className="flex flex-col items-center justify-center h-full px-6">
      {/* Quota info */}
      {quota && !isRecording && (
        <div className={`mb-4 px-3 py-1.5 rounded-full text-xs font-medium ${
          hasQuota ? 'bg-blue-50 text-blue-600' : 'bg-red-50 text-red-600'
        }`}>
          {hasQuota
            ? `${Math.round(quota.remaining_minutes)} min remaining`
            : 'Transcription quota exceeded'}
        </div>
      )}

      {/* Title input */}
      {!isRecording && (
        <div className="w-full max-w-sm mb-8">
          <input
            type="text"
            value={meetingTitle}
            onChange={(e) => setMeetingTitle(e.target.value)}
            placeholder="Meeting title (optional)"
            className="w-full px-4 py-3 border border-gray-300 rounded-xl text-center text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>
      )}

      {/* Duration display */}
      <div className="text-5xl font-light text-gray-900 mb-8 tabular-nums">
        {formatDuration(duration)}
      </div>

      {/* Error display */}
      {error && (
        <div className="flex items-center gap-2 mb-4 px-4 py-2 bg-red-50 rounded-lg max-w-sm">
          <AlertCircle className="w-4 h-4 text-red-500 flex-shrink-0" />
          <p className="text-sm text-red-600">{error}</p>
        </div>
      )}

      {/* Recording controls */}
      <div className="flex items-center gap-6">
        {isRecording ? (
          <>
            {/* Pause / Resume */}
            <button
              onClick={isPaused ? resumeRecording : pauseRecording}
              className="w-14 h-14 rounded-full bg-gray-100 flex items-center justify-center active:bg-gray-200"
            >
              {isPaused ? (
                <Play className="w-6 h-6 text-gray-700" />
              ) : (
                <Pause className="w-6 h-6 text-gray-700" />
              )}
            </button>

            {/* Stop */}
            <button
              onClick={handleStop}
              className="w-20 h-20 rounded-full bg-red-600 flex items-center justify-center active:bg-red-700 shadow-lg"
            >
              <Square className="w-8 h-8 text-white" fill="white" />
            </button>

            {/* Spacer for centering */}
            <div className="w-14" />
          </>
        ) : (
          /* Start recording */
          <button
            onClick={handleStart}
            disabled={!hasQuota}
            className="w-20 h-20 rounded-full bg-red-600 flex items-center justify-center active:bg-red-700 shadow-lg disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Mic className="w-8 h-8 text-white" />
          </button>
        )}
      </div>

      {/* Status text */}
      <p className="text-sm text-gray-500 mt-6">
        {isRecording
          ? isPaused
            ? 'Recording paused'
            : 'Recording in progress...'
          : !hasQuota
            ? 'Upgrade your plan to continue recording'
            : 'Tap to start recording'}
      </p>
    </div>
  )
}
