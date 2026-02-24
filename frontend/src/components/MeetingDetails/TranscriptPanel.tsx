"use client";

import { Transcript } from '@/types';
import { TranscriptView } from '@/components/TranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { TranscriptVersionSelector } from './TranscriptVersionSelector';
import { AudioPlayer } from './AudioPlayer';
import { useState } from 'react';
import { toast } from 'sonner';
import { authFetch } from '@/lib/api';
import { Loader2 } from 'lucide-react';
import type { DiarizationProgress } from '@/hooks/useDiarization';

interface TranscriptPanelProps {
  transcripts: Transcript[];
  onCopyTranscript: () => void;
  onDownloadRecording: () => Promise<void>;
  isRecording: boolean;
  currentVersion: number | undefined;
  onCurrentVersionChange: (version: number | undefined) => void;
  onDiarize?: () => void;
  onStopDiarize?: () => void; // NEW
  diarizationStatus?: string;
  isDiarizing?: boolean;
  diarizationProgress?: DiarizationProgress | null;
  diarizationWaitEstimate?: string | null;
  speakerMap?: { [label: string]: string };
  meetingId?: string; // Add meetingId
  onTranscriptsUpdate?: (transcripts: Transcript[]) => void; // Add update handler
}

export function TranscriptPanel({
  transcripts,
  onCopyTranscript,
  onDownloadRecording,
  isRecording,
  currentVersion,
  onCurrentVersionChange,
  onDiarize,
  onStopDiarize,
  diarizationStatus,
  isDiarizing,
  diarizationProgress,
  diarizationWaitEstimate,
  speakerMap,
  meetingId,
  onTranscriptsUpdate
}: TranscriptPanelProps) {
  const [isLoading, setIsLoading] = useState(false);

  const handleVersionChange = async (versionNum: number) => {
    if (!meetingId || !onTranscriptsUpdate) return;

    setIsLoading(true);
    try {
      // Handle switching to Live Transcript
      if (versionNum === -1) {
        const response = await authFetch(`/get-meeting/${meetingId}`);
        if (!response.ok) throw new Error('Failed to fetch live version');
        
        const data = await response.json();
        if (data.transcripts) {
          onTranscriptsUpdate(data.transcripts);
          onCurrentVersionChange(undefined);
          toast.success('Switched to Live Transcript');
        }
        return;
      }

      // Handle switching to specific version
      const response = await authFetch(`/meetings/${meetingId}/versions/${versionNum}`);
      if (!response.ok) throw new Error('Failed to fetch version');
      
      const data = await response.json();
      if (data.content) {
        // Map backend content to Transcript type if necessary, or assume it matches
        onTranscriptsUpdate(data.content);
        onCurrentVersionChange(versionNum);
        toast.success(`Switched to version ${versionNum}`);
      }
    } catch (error) {
      console.error('Error switching version:', error);
      toast.error('Failed to load transcript version');
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="hidden md:flex md:w-1/4 lg:w-1/3 min-w-0 border-r border-gray-200 bg-white flex-col relative shrink-0">
      {meetingId && <AudioPlayer meetingId={meetingId} />}
      {/* Title area */}
      <div className="p-4 border-b border-gray-200">
        {meetingId && onTranscriptsUpdate && (
          <div className="mb-4">
            <TranscriptVersionSelector 
              meetingId={meetingId} 
              onVersionChange={handleVersionChange}
              currentVersionNum={currentVersion ?? -1}
              refreshTrigger={diarizationStatus} // Pass status as trigger
            />
            {/* Alignment Legend - Moved inside header */}
            {(diarizationStatus === 'completed' || currentVersion !== undefined) && (
              <div className="mt-2 flex items-center gap-4 text-xs text-gray-500">
                <span className="font-medium">Legend:</span>
                <div className="flex items-center gap-1.5">
                  <div className="w-2 h-2 rounded-full bg-green-500"></div>
                  <span>Confident</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <div className="w-2 h-2 rounded-full bg-yellow-500"></div>
                  <span>Uncertain</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <div className="w-2 h-2 rounded-full bg-orange-500"></div>
                  <span>Overlap</span>
                </div>
              </div>
            )}
          </div>
        )}
        <TranscriptButtonGroup
          transcriptCount={transcripts?.length || 0}
          onCopyTranscript={onCopyTranscript}
          onDownloadRecording={onDownloadRecording}
          onDiarize={onDiarize}
          onStopDiarize={onStopDiarize}
          diarizationStatus={diarizationStatus}
          isDiarizing={isDiarizing}
          diarizationProgress={diarizationProgress}
          diarizationWaitEstimate={diarizationWaitEstimate}
          isRecording={isRecording}
        />
      </div>

      {/* Transcript content */}
      <div className="flex-1 overflow-y-auto pb-4 relative">
        {isLoading ? (
          <div className="absolute inset-0 bg-white/80 z-10 flex items-center justify-center">
            <Loader2 className="w-8 h-8 text-blue-500 animate-spin" />
          </div>
        ) : null}
        <TranscriptView 
          transcripts={transcripts} 
          speakerMap={speakerMap} 
          isRecording={isRecording}
          forceShowSpeakers={currentVersion !== undefined}
        />
      </div>
    </div>
  );
}
