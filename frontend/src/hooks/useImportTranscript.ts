'use client';

import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { applyPinnedSummaryLanguageToMeeting } from '@/lib/summary-language-preferences';

interface TranscriptImportResult {
  meeting_id: string;
  title: string;
  segments_count: number;
  duration_seconds: number;
}

/** Pick a .txt/.md/.srt/.vtt transcript, save it as a meeting, and open it. */
export function useImportTranscript() {
  const router = useRouter();
  const { refetchMeetings } = useSidebar();
  const [isImporting, setIsImporting] = useState(false);

  const importTranscript = useCallback(async () => {
    if (isImporting) return;
    setIsImporting(true);
    try {
      const result = await invoke<TranscriptImportResult | null>('import_transcript_command');
      if (!result) return; // dialog cancelled

      try {
        await applyPinnedSummaryLanguageToMeeting(result.meeting_id);
      } catch (error) {
        console.warn('Failed to apply pinned summary language to imported transcript:', error);
      }
      await refetchMeetings();
      toast.success(`Imported "${result.title}"`, {
        description: `${result.segments_count} transcript segments. Generate a summary from the meeting page.`,
      });
      router.push(`/meeting-details?id=${result.meeting_id}`);
    } catch (error) {
      toast.error('Transcript import failed', { description: String(error) });
    } finally {
      setIsImporting(false);
    }
  }, [isImporting, refetchMeetings, router]);

  return { importTranscript, isImporting };
}
