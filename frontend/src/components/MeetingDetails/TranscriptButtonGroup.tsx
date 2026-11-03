"use client";

import { useState, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, FolderOpen, RefreshCw, Users } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { RetranscribeDialog } from './RetranscribeDialog';
import { SpeakerRenameModal } from './SpeakerRenameModal';
import { useConfig } from '@/contexts/ConfigContext';

interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  speakers?: string[];
  showSpeakerModal?: boolean;
  onSpeakerModalOpenChange?: (open: boolean) => void;
}

export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  speakers = [],
  showSpeakerModal: controlledShowModal,
  onSpeakerModalOpenChange,
}: TranscriptButtonGroupProps) {
  const { betaFeatures } = useConfig();
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);
  const [internalShowSpeakerModal, setInternalShowSpeakerModal] = useState(false);

  const isSpeakerModalOpen = controlledShowModal !== undefined ? controlledShowModal : internalShowSpeakerModal;
  const setSpeakerModalOpen = (open: boolean) => {
    if (onSpeakerModalOpenChange) {
      onSpeakerModalOpenChange(open);
    } else {
      setInternalShowSpeakerModal(open);
    }
  };

  const handleRetranscribeComplete = useCallback(async () => {
    // Refetch transcripts to show the updated data
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  return (
    <div className="flex items-center justify-center w-full gap-2">
      <ButtonGroup>
        <Button
          variant="outline"
          size="sm"
          className="px-2 @[22rem]:px-3"
          onClick={() => {
            Analytics.trackButtonClick('copy_transcript', 'meeting_details');
            onCopyTranscript();
          }}
          disabled={transcriptCount === 0}
          title={transcriptCount === 0 ? 'No transcript available' : 'Copy Transcript'}
        >
          <Copy />
          <span className="hidden @[22rem]:inline">Copy</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          className="px-2 @[22rem]:px-4"
          onClick={() => {
            Analytics.trackButtonClick('open_recording_folder', 'meeting_details');
            onOpenMeetingFolder();
          }}
          title="Open Recording Folder"
        >
          <FolderOpen className="@[22rem]:mr-2" size={18} />
          <span className="hidden @[22rem]:inline">Recording</span>
        </Button>

        {meetingId && speakers.length > 0 && (
          <Button
            size="sm"
            variant="outline"
            className="px-2 @[22rem]:px-3.5 hover:bg-blue-50/60 hover:text-blue-700 hover:border-blue-200 transition-colors"
            onClick={() => {
              Analytics.trackButtonClick('open_speaker_rename_modal', 'meeting_details');
              setSpeakerModalOpen(true);
            }}
            title="Identify and rename detected speakers"
          >
            <Users className="@[22rem]:mr-1.5 text-blue-600" size={16} />
            <span className="hidden @[22rem]:inline font-medium">Speakers ({speakers.length})</span>
          </Button>
        )}

        {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
          <Button
            size="sm"
            variant="outline"
            className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 px-2 @[22rem]:px-4"
            onClick={() => {
              Analytics.trackButtonClick('enhance_transcript', 'meeting_details');
              setShowRetranscribeDialog(true);
            }}
            title="Retranscribe to enhance your recorded audio"
          >
            <RefreshCw className="@[22rem]:mr-2" size={18} />
            <span className="hidden @[22rem]:inline">Enhance</span>
          </Button>
        )}
      </ButtonGroup>

      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}

      {meetingId && speakers.length > 0 && (
        <SpeakerRenameModal
          open={isSpeakerModalOpen}
          onOpenChange={setSpeakerModalOpen}
          meetingId={meetingId}
          speakers={speakers}
          onComplete={onRefetchTranscripts}
        />
      )}
    </div>
  );
}
