'use client'

import React from 'react'
import MeetingDetail from '@/components/MeetingDetail'

export default function MeetingDetailPage({
  params,
}: {
  params: { id: string }
}) {
  return <MeetingDetail meetingId={params.id} />
}
