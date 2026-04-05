import { useCallback } from 'react';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import type { MeetingDetails } from '@/types/meeting';

interface UseMeetingOperationsProps {
  meeting: MeetingDetails;
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return 'Unknown error';
}

export function useMeetingOperations({
  meeting,
}: UseMeetingOperationsProps) {

  // Open meeting folder in file explorer
  const handleOpenMeetingFolder = useCallback(async () => {
    try {
      await invokeTauri('meeting_folder_open', { meetingId: meeting.id });
    } catch (error) {
      console.error('Failed to open meeting folder:', error);
      toast.error('Failed to open recording folder', {
        description: getErrorMessage(error),
      });
    }
  }, [meeting.id]);

  const handleExportMarkdown = useCallback(async () => {
    try {
      const result = await invokeTauri<{
        meeting_id: string;
        output_path?: string;
        wrote_file: boolean;
      }>('meeting_export_markdown', {
        meetingId: meeting.id,
      });

      if (result.wrote_file) {
        toast.success('Markdown exported successfully', {
          description: result.output_path || 'Export completed in meeting folder.',
        });
      } else {
        toast.info('Markdown preview generated');
      }
    } catch (error) {
      console.error('Failed to export meeting markdown:', error);
      toast.error('Failed to export markdown', {
        description: getErrorMessage(error),
      });
    }
  }, [meeting.id]);

  return {
    handleOpenMeetingFolder,
    handleExportMarkdown,
  };
}
