'use client'

import React from 'react'
import { useSearchParams } from 'next/navigation'
import MeetingDetail from '@/components/MeetingDetail'

export default function MeetingPage() {
  const searchParams = useSearchParams()
  const id = searchParams.get('id')

  if (!id) {
    return (
      <div className="flex items-center justify-center h-full text-iq-medium">
        <p>No meeting selected</p>
      </div>
    )
  }

  return <MeetingDetail meetingId={id} />
}
