"use client";

import { TranscriptView } from '@/components/TranscriptView';
import { MeetingSpeaker, SpeakerJobStatus, Transcript, TranscriptSegmentData } from '@/types';
import { SpeakerChip } from '@/components/Speakers/SpeakerChip';
import { SpeakerBar } from '@/components/Speakers/SpeakerBar';
import { SpeakerJobBanner } from '@/components/Speakers/SpeakerJobBanner';
import { useCallback, useMemo } from 'react';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';

export interface SpeakerTools {
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  editable: boolean;
  job: SpeakerJobStatus | null;
  onRename: (key: string, name: string) => Promise<void>;
  onMerge: (fromKey: string, intoKey: string) => Promise<void>;
  onReassign: (transcriptId: string, key: string | null) => Promise<void>;
  onCancelJob: () => Promise<void>;
  onStartIdentify: (numSpeakers: number | null) => Promise<void>;
}


interface TranscriptPanelProps {
  transcripts: Transcript[];
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  isRecording: boolean;
  disableAutoScroll?: boolean;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;

  speakerTools?: SpeakerTools;
}

export function TranscriptPanel({
  transcripts,
  customPrompt,
  onPromptChange,
  onCopyTranscript,
  onOpenMeetingFolder,
  isRecording,
  disableAutoScroll = false,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  speakerTools,
}: TranscriptPanelProps) {
  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker ?? null,
    }));
  }, [transcripts, usePagination, segments]);

  // Stable for a given speakerTools, so the memoised chips skip re-rendering while the list scrolls.
  const renderSpeaker = useCallback((segment: TranscriptSegmentData, isRunStart: boolean) => {
    if (!speakerTools || !segment.speaker) return null;
    // Rows inside a run only get the hover control, and only while editing is possible.
    if (!isRunStart && !speakerTools.editable) return null;
    return (
      <SpeakerChip
        compact={!isRunStart}
        speakerKey={segment.speaker}
        transcriptId={segment.id}
        speakers={speakerTools.speakers}
        names={speakerTools.names}
        editable={speakerTools.editable}
        onRename={speakerTools.onRename}
        onMerge={speakerTools.onMerge}
        onReassign={speakerTools.onReassign}
      />
    );
  }, [speakerTools]);

  return (
    <div className="flex h-full min-w-0 w-full bg-white flex-col relative @container">
      {/* Title area */}
      <div className="p-4 border-b border-gray-200">
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={onCopyTranscript}
          onOpenMeetingFolder={onOpenMeetingFolder}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={onRefetchTranscripts}
          onIdentifySpeakers={speakerTools?.onStartIdentify}
          hasSpeakers={(speakerTools?.speakers.length ?? 0) > 0}
          speakerJobActive={!!speakerTools?.job}
        />
      </div>

      {speakerTools && (
        <>
          <SpeakerJobBanner job={speakerTools.job} onCancel={() => void speakerTools.onCancelJob()} />
          <SpeakerBar speakers={speakerTools.speakers} names={speakerTools.names} editable={speakerTools.editable} onRename={speakerTools.onRename} />
        </>
      )}

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden pb-4">
        <VirtualizedTranscriptView
          segments={convertedSegments}
          isRecording={isRecording}
          isPaused={false}
          isProcessing={false}
          isStopping={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={disableAutoScroll}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
          renderSpeaker={speakerTools ? renderSpeaker : undefined}
        />
      </div>

      {/* Custom prompt input at bottom of transcript section */}
      {!isRecording && convertedSegments.length > 0 && (
        <div className="p-1 border-t border-gray-200">
          <textarea
            placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
            className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}
    </div>
  );
}
