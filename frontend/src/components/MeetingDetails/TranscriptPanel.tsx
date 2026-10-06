"use client";

import { MeetingSpeaker, Person, SpeakerJobStatus, Transcript, TranscriptSegmentData } from '@/types';
import { rowSpeakerControl } from '@/lib/speakers';
import { chooseSpeakerSample, followAlong, needsMoreRows, rowIndexAtTime, type FollowState } from '@/lib/playback';
import { fetchAllMeetingTranscripts } from '@/lib/transcripts';
import { convertTranscriptsToSegments } from '@/hooks/usePaginatedTranscripts';
import { SpeakerChip } from '@/components/Speakers/SpeakerChip';
import { SpeakerBar } from '@/components/Speakers/SpeakerBar';
import { SpeakerJobBanner } from '@/components/Speakers/SpeakerJobBanner';
import { usePlayback } from '@/hooks/usePlayback';
import { PlayerBar } from './PlayerBar';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import { VirtualizedTranscriptView, type RenderSpeaker } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';

export interface SpeakerTools {
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  editable: boolean;
  /** Known people for the name autocomplete */
  people: Person[];
  onRename: (key: string, name: string) => Promise<void>;
  onMerge: (fromKey: string, intoKey: string) => Promise<void>;
  onReassign: (transcriptId: string, key: string | null) => Promise<void>;
  /** The automatic name, else the suggestion, becomes a typed name. */
  onConfirm: (key: string) => Promise<void>;
  /** "Not <name>": never proposed again for this speaker. */
  onReject: (key: string) => Promise<void>;
  onCancelJob: () => Promise<void>;
  onStartIdentify: (numSpeakers: number | null) => Promise<void>;
  /** Queues the naming stage; progress shows in the job banner. */
  onGuessNames: () => Promise<void>;
  /** Plays a few seconds of the speaker's longest line. */
  onPlaySample: (key: string) => void;
}

/** What the meeting page provides; the panel adds the sample player, which it owns. */
export type SpeakerToolsInput = Omit<SpeakerTools, 'onPlaySample'>;

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

  speakerTools?: SpeakerToolsInput;
  /** Kept out of speakerTools: it changes on every progress event, and the rows must not. */
  speakerJob?: SpeakerJobStatus | null;
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
  speakerJob = null,
}: TranscriptPanelProps) {
  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return convertTranscriptsToSegments(transcripts);
  }, [transcripts, usePagination, segments]);

  // The recording plays in the panel; meetings without a folder have no audio.
  const playback = usePlayback(meetingId ?? null, !isRecording && !!meetingId && !!meetingFolderPath);

  // Handlers read the latest controls through this ref, so they keep their identity while the
  // clock moves and the memoised rows do not re-render.
  const playbackRef = useRef(playback);
  playbackRef.current = playback;

  // Follow along: the row playing now is highlighted and kept in view until the user scrolls away.
  const [follow, setFollow] = useState<FollowState>({ following: true, showBack: false });
  useEffect(() => {
    if (!playback.playing) setFollow((state) => followAlong(state, 'paused'));
  }, [playback.playing]);
  const activeIndex = playback.playing ? rowIndexAtTime(convertedSegments, playback.clockS) : -1;
  const activeSegmentId = follow.following && activeIndex >= 0 ? convertedSegments[activeIndex].id : null;
  const onPlayFrom = useCallback((startS: number) => {
    setFollow((state) => followAlong(state, 'play-from-row'));
    playbackRef.current.playFrom(startS);
  }, []);
  const onManualScroll = useCallback(() => {
    if (playbackRef.current.playing) setFollow((state) => followAlong(state, 'manual-scroll'));
  }, []);
  // Playback past the last loaded row loads the next page.
  const needsMore = playback.playing && needsMoreRows(convertedSegments, playback.clockS, !!hasMore);
  useEffect(() => {
    if (needsMore && !isLoadingMore) onLoadMore?.();
  }, [needsMore, isLoadingMore, onLoadMore, convertedSegments.length]);

  // Samples need every row (the view only holds the loaded pages): fetched on the first sample,
  // again after speakers change.
  const allRowsRef = useRef<TranscriptSegmentData[] | null>(null);
  const speakerList = speakerTools?.speakers;
  useEffect(() => {
    allRowsRef.current = null;
  }, [speakerList]);
  const playSample = useCallback(async (key: string) => {
    if (!meetingId) return;
    try {
      const rows = allRowsRef.current ?? convertTranscriptsToSegments(await fetchAllMeetingTranscripts(meetingId));
      allRowsRef.current = rows;
      const sample = chooseSpeakerSample(rows, key);
      if (!sample) {
        toast.info('No line of this speaker to play');
        return;
      }
      playbackRef.current.playFrom(sample.startS, sample.endS);
    } catch (error) {
      console.error('Failed to play a speaker sample:', error);
      toast.error('Failed to play a sample');
    }
  }, [meetingId]);
  const tools = useMemo<SpeakerTools | undefined>(
    () => speakerTools && { ...speakerTools, onPlaySample: (key: string) => { void playSample(key); } },
    [speakerTools, playSample],
  );

  // Stable for a given speakerTools (which leaves out the job and the clock), so the memoised
  // rows skip re-rendering while the list scrolls, a job reports progress or playback moves.
  const renderSpeaker = useCallback<RenderSpeaker>((speakerKey, transcriptId, isRunStart) => {
    if (!tools) return null;
    const control = rowSpeakerControl(speakerKey, isRunStart, tools.editable);
    if (!control) return null;
    return (
      <SpeakerChip
        compact={control.compact}
        speakerKey={control.speakerKey}
        transcriptId={transcriptId}
        speakers={tools.speakers}
        names={tools.names}
        editable={tools.editable}
        people={tools.people}
        onRename={tools.onRename}
        onMerge={tools.onMerge}
        onReassign={tools.onReassign}
        onConfirm={tools.onConfirm}
        onReject={tools.onReject}
        onPlaySample={playback.ready ? tools.onPlaySample : undefined}
      />
    );
  }, [tools, playback.ready]);

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
          onIdentifySpeakers={tools?.onStartIdentify}
          hasSpeakers={(tools?.speakers.length ?? 0) > 0}
          speakerJobActive={!!speakerJob}
        />
      </div>

      {tools && (
        <>
          <SpeakerJobBanner job={speakerJob} onCancel={() => void tools.onCancelJob()} />
          <SpeakerBar
            speakers={tools.speakers}
            names={tools.names}
            editable={tools.editable}
            people={tools.people}
            onRename={tools.onRename}
            onConfirm={tools.onConfirm}
            onReject={tools.onReject}
            onGuessNames={tools.onGuessNames}
            onPlaySample={playback.ready ? tools.onPlaySample : undefined}
          />
        </>
      )}

      {/* Transcript content - use virtualized view for better performance */}
      <div className="relative flex-1 overflow-hidden pb-4">
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
          renderSpeaker={tools ? renderSpeaker : undefined}
          activeSegmentId={activeSegmentId}
          onPlayFrom={playback.ready ? onPlayFrom : undefined}
          onManualScroll={onManualScroll}
        />
        {follow.showBack && playback.playing && (
          <button
            type="button"
            className="absolute bottom-6 left-1/2 -translate-x-1/2 rounded-full bg-blue-600 px-3 py-1 text-xs font-medium text-white shadow hover:bg-blue-700"
            onClick={() => setFollow((state) => followAlong(state, 'back'))}
          >
            Back to playback
          </button>
        )}
      </div>

      <PlayerBar playback={playback} />

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
