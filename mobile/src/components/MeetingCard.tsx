'use client'

import React from 'react'
import { format } from 'date-fns'
import { Meeting } from '@/types'
import { Clock, CloudOff, Loader2, CheckCircle2, AlertCircle } from 'lucide-react'

interface MeetingCardProps {
  meeting: Meeting
}

const statusConfig: Record<string, { label: string; color: string; icon: React.ElementType }> = {
  recording: { label: 'Recording', color: 'text-iq-red', icon: Loader2 },
  pending_upload: { label: 'Pending upload', color: 'text-iq-orange', icon: CloudOff },
  uploading: { label: 'Uploading', color: 'text-iq-blue', icon: Loader2 },
  transcribing: { label: 'Transcribing', color: 'text-iq-blue', icon: Loader2 },
  summarizing: { label: 'Summarizing', color: 'text-iq-blue', icon: Loader2 },
  completed: { label: 'Completed', color: 'text-iq-green', icon: CheckCircle2 },
  error: { label: 'Error', color: 'text-iq-red', icon: AlertCircle },
}

export default function MeetingCard({ meeting }: MeetingCardProps) {
  const config = statusConfig[meeting.status] || statusConfig.completed
  const StatusIcon = config.icon
  const isAnimated = ['recording', 'uploading', 'transcribing', 'summarizing'].includes(meeting.status)

  const durationStr = meeting.duration_seconds
    ? `${Math.floor(meeting.duration_seconds / 60)}m ${Math.round(meeting.duration_seconds % 60)}s`
    : null

  return (
    <div className="bg-white border border-iq-light-shade rounded-iq-lg p-4 active:bg-iq-light">
      <div className="flex items-start justify-between">
        <div className="flex-1 min-w-0">
          <h3 className="text-sm font-semibold text-iq-dark truncate">
            {meeting.title || 'Untitled Meeting'}
          </h3>
          <p className="text-xs text-iq-medium mt-0.5">
            {format(new Date(meeting.created_at), 'MMM d, yyyy h:mm a')}
          </p>
        </div>
        <div className={`flex items-center gap-1 ${config.color}`}>
          <StatusIcon className={`w-3.5 h-3.5 ${isAnimated ? 'animate-spin' : ''}`} />
          <span className="text-xs font-medium">{config.label}</span>
        </div>
      </div>
      {durationStr && (
        <div className="flex items-center gap-1 mt-2 text-xs text-iq-medium">
          <Clock className="w-3 h-3" />
          <span>{durationStr}</span>
        </div>
      )}
      {meeting.sync_status === 'local_only' && (
        <div className="flex items-center gap-1 mt-1 text-xs text-iq-orange">
          <CloudOff className="w-3 h-3" />
          <span>Not synced</span>
        </div>
      )}
    </div>
  )
}
