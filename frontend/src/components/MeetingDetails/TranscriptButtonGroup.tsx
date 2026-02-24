"use client";

import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, Download, Users, Loader2 } from 'lucide-react';
import Analytics from '@/lib/analytics';
import type { DiarizationProgress } from '@/hooks/useDiarization';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onDownloadRecording: () => Promise<void>;
  onDiarize?: () => void;
  onStopDiarize?: () => void; // NEW
  diarizationStatus?: string;
  isDiarizing?: boolean;
  isRecording?: boolean;
  diarizationProgress?: DiarizationProgress | null;
  diarizationWaitEstimate?: string | null;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onDownloadRecording,
  onDiarize,
  onStopDiarize,
  diarizationStatus,
  isDiarizing,
  isRecording,
  diarizationProgress: _diarizationProgress,
  diarizationWaitEstimate: _diarizationWaitEstimate
}: TranscriptButtonGroupProps) {
  const isProcessing = diarizationStatus === 'processing' || isDiarizing;

  return (
    <div className="flex items-start justify-between w-full gap-3">
      <div className="flex flex-col gap-1 min-w-0">
        <div className="flex gap-2">
        {onDiarize && (
          <Button
            size="sm"
            variant={isProcessing ? "destructive" : (diarizationStatus === 'completed' ? "outline" : "default")}
            className={isProcessing ? "" : (diarizationStatus === 'completed' ? "" : "bg-indigo-600 hover:bg-indigo-700 text-white")}
            onClick={isProcessing ? onStopDiarize : onDiarize}
            disabled={(!isProcessing && isRecording) || (!isProcessing && !onDiarize) || (isProcessing && !onStopDiarize)}
          >
            {isProcessing ? (
               // No spinner for Stop button usually, but here we want to show it's working until stopped?
               // Actually, "Stop" action should be immediate.
               <Users className="mr-2 h-4 w-4" /> 
            ) : (
               <Users className="mr-2 h-4 w-4" />
            )}
            <span className="hidden lg:inline">
              {isProcessing ? 'Stop Identification' :
                (diarizationStatus === 'completed' ? 'Re-identify Speakers' : 
                 (diarizationStatus === 'failed' ? 'Failed (Retry)' : 
                  (diarizationStatus === 'stopped' ? 'Stopped (Retry)' : 'Identify Speakers')))}
            </span>
          </Button>
        )}
        </div>
      </div>

      <div className="flex flex-col items-end gap-1">
        <ButtonGroup >
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              Analytics.trackButtonClick('copy_transcript', 'meeting_details');
              onCopyTranscript();
            }}
            disabled={transcriptCount === 0}
            title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
          >
            <Copy className="h-4 w-4 lg:mr-2" />
            <span className="hidden lg:inline">Copy</span>
          </Button>

          <Button
            size="sm"
            variant="outline"
            className="xl:px-4"
            onClick={() => {
              Analytics.trackButtonClick('download_recording', 'meeting_details');
              onDownloadRecording();
            }}
            title="Download Audio File"
          >
            <Download className="h-4 w-4 lg:mr-2" />
            <span className="hidden lg:inline">Download Recording</span>
          </Button>
        </ButtonGroup>

        {isProcessing && (
          <div className="flex items-center gap-2 text-xs text-slate-600">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            <span>Diarization in progress. Usually takes 3-5 minutes.</span>
          </div>
        )}
      </div>
    </div>
  );
}
