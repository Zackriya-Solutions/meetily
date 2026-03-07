import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { PermissionWarning } from '@/components/PermissionWarning';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, GlobeIcon } from 'lucide-react';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { ModalType } from '@/hooks/useModalState';
import { useIsLinux } from '@/hooks/usePlatform';
import { useMemo, useEffect } from 'react';
import { useSpeakers } from '@/contexts/SpeakerContext';

/**
 * TranscriptPanel Component
 *
 * Displays transcript content with controls for copying and language settings.
 * Uses TranscriptContext, ConfigContext, and RecordingStateContext internally.
 */

interface TranscriptPanelProps {
  // indicates stop-processing state for transcripts; derived from backend statuses.
  isProcessingStop: boolean;
  isStopping: boolean;
  showModal: (name: ModalType, message?: string) => void;
}

export function TranscriptPanel({
  isProcessingStop,
  isStopping,
  showModal
}: TranscriptPanelProps) {
  // Contexts
  const { transcripts, transcriptContainerRef, copyTranscript, currentMeetingId } = useTranscripts();
  const { transcriptModelConfig, liveTranscription } = useConfig();
  const { isRecording, isPaused } = useRecordingState();
  const { checkPermissions, isChecking, hasSystemAudio, hasMicrophone } = usePermissionCheck();
  const isLinux = useIsLinux();
  const { speakerMap, renameSpeaker, setMeetingId } = useSpeakers();

  // Keep SpeakerContext in sync with the current recording's meeting ID
  useEffect(() => {
    setMeetingId(currentMeetingId ?? null);
  }, [currentMeetingId, setMeetingId]);

  // Convert transcripts to segments for virtualized view.
  // When liveTranscription is disabled and recording is active, pass no segments so
  // live transcript updates are hidden on-screen. The backend still transcribes and
  // saves everything — this is purely a display toggle.
  const segments = useMemo(() => {
    if (isRecording && !liveTranscription) return [];
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker,
    }));
  }, [transcripts, isRecording, liveTranscription]);

  return (
    <div ref={transcriptContainerRef} className="w-full border-r border-gray-200 dark:border-border bg-white dark:bg-card flex flex-col overflow-y-auto">
      {/* Title area - Sticky header */}
      <div className="sticky top-0 z-10 bg-white dark:bg-card p-4 border-gray-200 dark:border-border">
        <div className="flex flex-col space-y-3">
          <div className="flex  flex-col space-y-2">
            <div className="flex justify-center  items-center space-x-2">
              <ButtonGroup>
                {transcripts?.length > 0 && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={copyTranscript}
                    title="Copy Transcript"
                  >
                    <Copy />
                    <span className='hidden md:inline'>
                      Copy
                    </span>
                  </Button>
                )}
                {transcriptModelConfig.provider === "localWhisper" &&
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => showModal('languageSettings')}
                    title="Language"
                  >
                    <GlobeIcon />
                    <span className='hidden md:inline'>
                      Language
                    </span>
                  </Button>
                }
              </ButtonGroup>
            </div>
          </div>
        </div>
      </div>

      {/* Permission Warning - Not needed on Linux */}
      {!isRecording && !isChecking && !isLinux && (
        <div className="flex justify-center px-4 pt-4">
          <PermissionWarning
            hasMicrophone={hasMicrophone}
            hasSystemAudio={hasSystemAudio}
            onRecheck={checkPermissions}
            isRechecking={isChecking}
          />
        </div>
      )}

      {/* Transcript content */}
      <div className="pb-20">
        <div className="flex justify-center">
          <div className="w-2/3 max-w-[750px]">
            {/* Show a silent-recording notice when live display is off */}
            {isRecording && !liveTranscription && (
              <div className="flex flex-col items-center justify-center mt-12 text-center gap-3 px-4">
                <div className="relative flex items-center justify-center w-10 h-10">
                  <div className="w-3 h-3 rounded-full bg-red-500" />
                  <div className="absolute w-6 h-6 rounded-full bg-red-500/20 animate-ping" />
                </div>
                <p className="text-sm font-medium text-gray-700">Recording in progress</p>
                <p className="text-xs text-gray-400 max-w-xs leading-relaxed">
                  Live transcription display is turned off. Your audio is still being transcribed and will be fully available once the recording ends.
                </p>
              </div>
            )}
            <VirtualizedTranscriptView
              segments={segments}
              isRecording={isRecording}
              isPaused={isPaused}
              isProcessing={isProcessingStop}
              isStopping={isStopping}
              enableStreaming={isRecording && liveTranscription}
              showConfidence={true}
              speakerMap={speakerMap}
              onRenameSpeaker={currentMeetingId ? renameSpeaker : undefined}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
