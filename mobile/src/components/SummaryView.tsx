'use client'

import React from 'react'
import { SummaryDataResponse, MeetingStatus, Block, Section } from '@/types'
import { Sparkles } from 'lucide-react'

interface SummaryViewProps {
  summary?: SummaryDataResponse
  meetingId: string
  status: MeetingStatus
  onGenerateSummary?: () => void
  isGenerating?: boolean
}

export default function SummaryView({
  summary,
  meetingId,
  status,
  onGenerateSummary,
  isGenerating,
}: SummaryViewProps) {
  if (status === 'summarizing' || isGenerating) {
    return (
      <div className="flex flex-col items-center justify-center h-48 text-center px-6">
        <div className="animate-spin rounded-full h-6 w-6 border-b-2 border-blue-600 mb-3" />
        <p className="text-gray-500 text-sm">Generating summary...</p>
      </div>
    )
  }

  if (!summary || !summary._section_order || summary._section_order.length === 0) {
    const canGenerate = status === 'completed'

    return (
      <div className="flex flex-col items-center justify-center h-48 text-center px-6">
        <p className="text-gray-400 text-sm mb-3">
          {canGenerate
            ? 'No summary generated yet.'
            : 'Summary will be available after transcription completes.'}
        </p>
        {canGenerate && onGenerateSummary && (
          <button
            onClick={onGenerateSummary}
            className="flex items-center gap-1.5 px-4 py-2 bg-blue-600 text-white rounded-lg text-sm font-medium"
          >
            <Sparkles className="w-4 h-4" />
            Generate Summary
          </button>
        )}
      </div>
    )
  }

  // Render summary sections in order
  return (
    <div className="px-4 py-4 space-y-6">
      {summary._section_order.map((key) => {
        const section = summary[key] as Section | undefined
        if (!section || !section.title) return null

        return (
          <div key={key}>
            <h3 className="text-sm font-semibold text-gray-900 mb-2">{section.title}</h3>
            <div className="space-y-2">
              {section.blocks?.map((block: Block, i: number) => (
                <div key={block.id || i} className="text-sm text-gray-700 leading-relaxed">
                  {block.content}
                </div>
              ))}
            </div>
          </div>
        )
      })}
    </div>
  )
}
