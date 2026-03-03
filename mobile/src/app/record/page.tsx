'use client'

import React from 'react'
import { useAuth } from '@/contexts/AuthContext'
import RecordingScreen from '@/components/RecordingScreen'
import AuthPrompt from '@/components/AuthPrompt'

export default function RecordPage() {
  const { isAuthenticated, isLoading } = useAuth()

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600" />
      </div>
    )
  }

  if (!isAuthenticated) {
    return <AuthPrompt />
  }

  return <RecordingScreen />
}
