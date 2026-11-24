"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { TranscriptView } from '@/components/TranscriptView';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { OtherSpeakerName } from './OtherSpeakerName';
import { MeetingAudioPlayer } from './MeetingAudioPlayer';
import { SpeakerNameEditor } from './SpeakerNameEditor';
import { SpeakerAccuracy } from './SpeakerAccuracy';
import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';

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

  // Names of the other side, when found in the transcript
  othersLabel?: string;
  otherSpeakerNames?: string[];
  onSaveOtherSpeakerNames?: (names: string[]) => Promise<void>;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  /** Applies saved names in place; without it the panel refetches. */
  onSpeakerNamesChanged?: (transcriptIds: string[], name: string | null) => void;
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
  othersLabel,
  otherSpeakerNames,
  onSaveOtherSpeakerNames,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  onSpeakerNamesChanged,
}: TranscriptPanelProps) {
  const [speakerToName, setSpeakerToName] = useState<TranscriptSegmentData | null>(null);
  // Names recognized from earlier meetings, by voice label.
  const [speakerGuesses, setSpeakerGuesses] = useState<Record<string, string>>({});
  const [namesSaved, setNamesSaved] = useState(0);
  useEffect(() => setSpeakerGuesses({}), [meetingId]);
  // Refetched after each save: the backend drops voices that now have a name.
  useEffect(() => {
    if (!meetingId) return;
    let current = true;
    invoke<Record<string, string>>('get_meeting_speaker_guesses', { meetingId })
      .then(guesses => { if (current) setSpeakerGuesses(guesses); })
      .catch(error => console.error('Failed to load speaker guesses:', error));
    return () => { current = false; };
  }, [meetingId, namesSaved]);
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
      speaker: t.speaker,
      speakerLabel: t.speaker_label,
      speakerName: t.speaker_name,
    }));
  }, [transcripts, usePagination, segments]);

  // Guesses for unnamed voices on the loaded pages, so Dor can check them
  // before confirming; confirm touches only these.
  const pendingGuesses = useMemo(() => {
    const heard = new Set(convertedSegments.map(s => s.speakerLabel));
    return Object.entries(speakerGuesses).filter(([label]) => heard.has(label));
  }, [convertedSegments, speakerGuesses]);
  const [confirming, setConfirming] = useState(false);
  const confirmGuesses = async () => {
    if (!meetingId) return;
    setConfirming(true);
    try {
      const changed = await invoke<Record<string, string[]>>('confirm_speaker_guesses', {
        meetingId,
        speakerLabels: pendingGuesses.map(([label]) => label),
      });
      if (onSpeakerNamesChanged) Object.entries(changed).forEach(([name, ids]) => onSpeakerNamesChanged(ids, name));
      else await onRefetchTranscripts?.();
      setNamesSaved(n => n + 1);
    } catch (error) {
      console.error('Failed to confirm speaker names:', error);
      toast.error(`Could not save the names: ${String(error)}`);
    } finally {
      setConfirming(false);
    }
  };

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
        />
        {meetingId && !isRecording && <MeetingAudioPlayer meetingId={meetingId} />}
        {onSaveOtherSpeakerNames && convertedSegments.some(s => s.speaker === 'system') && (
          <OtherSpeakerName names={otherSpeakerNames ?? []} onSave={onSaveOtherSpeakerNames} />
        )}
        {meetingId && !isRecording && pendingGuesses.length > 0 && (
          <div className="mt-2 flex items-center justify-between gap-2 rounded-md bg-blue-50 px-3 py-2 text-sm text-blue-900">
            <span>
              {`Recognized from earlier meetings: ${pendingGuesses.map(([label, name]) => `${name} (${label})`).join(', ')}.`}
            </span>
            <Button size="sm" disabled={confirming} onClick={() => void confirmGuesses()}>
              {pendingGuesses.length === 1 ? 'Confirm name' : 'Confirm names'}
            </Button>
          </div>
        )}
        {meetingId && !isRecording && <SpeakerAccuracy meetingId={meetingId} refreshKey={namesSaved} />}
      </div>

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
          othersLabel={othersLabel}
          speakerGuesses={speakerGuesses}
          onEditSpeaker={meetingId && !isRecording ? setSpeakerToName : undefined}
          disableAutoScroll={disableAutoScroll}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
        />
      </div>

      {meetingId && (
        <SpeakerNameEditor
          meetingId={meetingId}
          segment={speakerToName}
          guess={speakerToName?.speakerLabel ? speakerGuesses[speakerToName.speakerLabel] : undefined}
          suggestions={otherSpeakerNames ?? []}
          onClose={() => setSpeakerToName(null)}
          onSaved={async (transcriptIds, name) => {
            // In place, so a long meeting keeps its loaded pages and scroll.
            if (onSpeakerNamesChanged) onSpeakerNamesChanged(transcriptIds, name);
            else await onRefetchTranscripts?.();
            setNamesSaved(n => n + 1);
          }}
          onGuessDismissed={label => setSpeakerGuesses(({ [label]: _, ...rest }) => rest)}
        />
      )}

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
