'use client'

import React from 'react'
import MeetingDetail from '@/components/MeetingDetail'

// Required for output: 'export' with dynamic routes
export function generateStaticParams() {
  return [] // Meeting IDs are created at runtime
}

export default function MeetingDetailPage({
  params,
}: {
  params: { id: string }
}) {
  return <MeetingDetail meetingId={params.id} />
}
