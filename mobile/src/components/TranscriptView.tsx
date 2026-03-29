'use client'

import React from 'react'
import { TranscriptSegment } from '@/types'

interface TranscriptViewProps {
  text?: string
  segments?: TranscriptSegment[]
}

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

export default function TranscriptView({ text, segments }: TranscriptViewProps) {
  if (!text && (!segments || segments.length === 0)) {
    return (
      <div className="flex flex-col items-center justify-center h-48 text-center px-6">
        <p className="text-iq-medium text-sm">
          No transcript available yet.
        </p>
        <p className="text-iq-medium text-xs mt-1">
          Transcript will appear here once audio is uploaded and processed.
        </p>
      </div>
    )
  }

  // If we have segments, show them with timestamps
  if (segments && segments.length > 0) {
    return (
      <div className="px-4 py-4 space-y-3">
        {segments.map((segment, i) => (
          <div key={i} className="flex gap-3">
            <span className="text-xs text-iq-medium font-mono mt-0.5 shrink-0 w-12">
              {formatTime(segment.start)}
            </span>
            <p className="text-sm text-iq-dark leading-relaxed">{segment.text}</p>
          </div>
        ))}
      </div>
    )
  }

  // Fallback: show raw text
  return (
    <div className="px-4 py-4">
      <p className="text-sm text-iq-dark leading-relaxed whitespace-pre-wrap">{text}</p>
    </div>
  )
}
