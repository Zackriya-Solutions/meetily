import { useCallback, RefObject } from 'react';
import { Transcript, Summary } from '@/types';
import { BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { generatePdfFromMarkdown } from '@/lib/pdfExport';
import { exportToOutline, loadOutlineConfig } from '@/lib/outlineExport';
import { copyToClipboard } from '@/lib/clipboard';

interface UseCopyOperationsProps {
  meeting: any;
  transcripts: Transcript[];
  meetingTitle: string;
  aiSummary: Summary | null;
  blockNoteSummaryRef: RefObject<BlockNoteSummaryViewRef>;
}

export function useCopyOperations({
  meeting,
  transcripts,
  meetingTitle,
  aiSummary,
  blockNoteSummaryRef,
}: UseCopyOperationsProps) {

  // Helper function to fetch ALL transcripts for copying (not just paginated data)
  const fetchAllTranscripts = useCallback(async (meetingId: string): Promise<Transcript[]> => {
    try {
      console.log('📊 Fetching all transcripts for copying:', meetingId);

      // First, get total count by fetching first page
      const firstPage = await invokeTauri('api_get_meeting_transcripts', {
        meetingId,
        limit: 1,
        offset: 0,
      }) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

      const totalCount = firstPage.total_count;
      console.log(`📊 Total transcripts in database: ${totalCount}`);

      if (totalCount === 0) {
        return [];
      }

      // Fetch all transcripts in one call
      const allData = await invokeTauri('api_get_meeting_transcripts', {
        meetingId,
        limit: totalCount,
        offset: 0,
      }) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

      console.log(`✅ Fetched ${allData.transcripts.length} transcripts from database for copying`);
      return allData.transcripts;
    } catch (error) {
      console.error('❌ Error fetching all transcripts:', error);
      toast.error('Failed to fetch transcripts for copying');
      return [];
    }
  }, []);

  // Copy transcript to clipboard
  const handleCopyTranscript = useCallback(async () => {
    // CHANGE: Fetch ALL transcripts from database, not from pagination state
    console.log('📊 Fetching all transcripts for copying...');
    const allTranscripts = await fetchAllTranscripts(meeting.id);

    if (!allTranscripts.length) {
      const error_msg = 'No transcripts available to copy';
      console.log(error_msg);
      toast.error(error_msg);
      return;
    }

    console.log(`✅ Copying ${allTranscripts.length} transcripts to clipboard`);

    // Format timestamps as recording-relative [MM:SS] instead of wall-clock time
    const formatTime = (seconds: number | undefined, fallbackTimestamp: string): string => {
      if (seconds === undefined) {
        // For old transcripts without audio_start_time, use wall-clock time
        return fallbackTimestamp;
      }
      const totalSecs = Math.floor(seconds);
      const mins = Math.floor(totalSecs / 60);
      const secs = totalSecs % 60;
      return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
    };

    const speakerPrefix = (speaker?: Transcript['speaker']): string => {
      if (speaker === 'mic') return '[You] ';
      if (speaker === 'system') return '[System] ';
      return '';
    };

    const header = `# Transcript of the Meeting: ${meeting.id} - ${meetingTitle ?? meeting.title}\n\n`;
    const date = `## Date: ${new Date(meeting.created_at).toLocaleDateString()}\n\n`;
    const fullTranscript = allTranscripts
      .map(t => `${formatTime(t.audio_start_time, t.timestamp)} ${speakerPrefix(t.speaker)}${t.text}  `)
      .join('\n');

    await copyToClipboard(header + date + fullTranscript);
    toast.success("Transcript copied to clipboard");

    // Track copy analytics
    const wordCount = allTranscripts
      .map(t => t.text.split(/\s+/).length)
      .reduce((a, b) => a + b, 0);

    await Analytics.trackCopy('transcript', {
      meeting_id: meeting.id,
      transcript_length: allTranscripts.length.toString(),
      word_count: wordCount.toString()
    });
  }, [meeting, meetingTitle, fetchAllTranscripts]);

  // Copy summary to clipboard
  const handleCopySummary = useCallback(async () => {
    try {
      let summaryMarkdown = '';

      console.log('🔍 Copy Summary - Starting...');

      // Try to get markdown from BlockNote editor first
      if (blockNoteSummaryRef.current?.getMarkdown) {
        console.log('📝 Trying to get markdown from ref...');
        summaryMarkdown = await blockNoteSummaryRef.current.getMarkdown();
        console.log('📝 Got markdown from ref, length:', summaryMarkdown.length);
      }

      // Fallback: Check if aiSummary has markdown property
      if (!summaryMarkdown && aiSummary && 'markdown' in aiSummary) {
        console.log('📝 Using markdown from aiSummary');
        summaryMarkdown = (aiSummary as any).markdown || '';
        console.log('📝 Markdown from aiSummary, length:', summaryMarkdown.length);
      }

      // Fallback: Check for legacy format
      if (!summaryMarkdown && aiSummary) {
        console.log('📝 Converting legacy format to markdown');
        const sections = Object.entries(aiSummary)
          .filter(([key]) => {
            // Skip non-section keys
            return key !== 'markdown' && key !== 'summary_json' && key !== '_section_order' && key !== 'MeetingName';
          })
          .map(([, section]) => {
            if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
              const sectionTitle = `## ${section.title}\n\n`;
              const sectionContent = section.blocks
                .map((block: any) => `- ${block.content}`)
                .join('\n');
              return sectionTitle + sectionContent;
            }
            return '';
          })
          .filter(s => s.trim())
          .join('\n\n');
        summaryMarkdown = sections;
        console.log('📝 Converted legacy format, length:', summaryMarkdown.length);
      }

      // If still no summary content, show message
      if (!summaryMarkdown.trim()) {
        console.error('❌ No summary content available to copy');
        toast.error('No summary content available to copy');
        return;
      }

      // Build metadata header
      const header = `# Meeting Summary: ${meetingTitle}\n\n`;
      const metadata = `**Meeting ID:** ${meeting.id}\n**Date:** ${new Date(meeting.created_at).toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      })}\n**Copied on:** ${new Date().toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      })}\n\n---\n\n`;

      const fullMarkdown = header + metadata + summaryMarkdown;
      await copyToClipboard(fullMarkdown);

      console.log('✅ Successfully copied to clipboard!');
      toast.success("Summary copied to clipboard");

      // Track copy analytics
      await Analytics.trackCopy('summary', {
        meeting_id: meeting.id,
        has_markdown: (!!aiSummary && 'markdown' in aiSummary).toString()
      });
    } catch (error) {
      console.error('❌ Failed to copy summary:', error);
      toast.error("Failed to copy summary");
    }
  }, [aiSummary, meetingTitle, meeting, blockNoteSummaryRef]);

  // Export summary as PDF
  const handleExportPdf = useCallback(async () => {
    try {
      let summaryMarkdown = '';

      // Try to get markdown from BlockNote editor first
      if (blockNoteSummaryRef.current?.getMarkdown) {
        summaryMarkdown = await blockNoteSummaryRef.current.getMarkdown();
      }

      // Fallback: Check if aiSummary has markdown property
      if (!summaryMarkdown && aiSummary && 'markdown' in aiSummary) {
        summaryMarkdown = (aiSummary as any).markdown || '';
      }

      // Fallback: Convert legacy format
      if (!summaryMarkdown && aiSummary) {
        const sections = Object.entries(aiSummary)
          .filter(([key]) => key !== 'markdown' && key !== 'summary_json' && key !== '_section_order' && key !== 'MeetingName')
          .map(([, section]) => {
            if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
              const sectionTitle = `## ${section.title}\n\n`;
              const sectionContent = section.blocks
                .map((block: any) => `- ${block.content}`)
                .join('\n');
              return sectionTitle + sectionContent;
            }
            return '';
          })
          .filter(s => s.trim())
          .join('\n\n');
        summaryMarkdown = sections;
      }

      if (!summaryMarkdown.trim()) {
        toast.error('No summary content available to export');
        return;
      }

      // Generate the PDF bytes
      const dateStr = new Date(meeting.created_at).toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
      });
      const bytes = await generatePdfFromMarkdown(summaryMarkdown, meetingTitle, dateStr);

      // Sanitize the title for use as a filename
      const safeTitle = meetingTitle.replace(/[<>:"/\\|?*]/g, '_').trim() || 'meeting-summary';

      // Open native save dialog
      const filePath = await save({
        defaultPath: `${safeTitle}.pdf`,
        filters: [{ name: 'PDF Document', extensions: ['pdf'] }],
      });

      if (!filePath) {
        return;
      }

      // Write the file via Rust command
      await invokeTauri('write_bytes_to_file', { path: filePath, data: Array.from(bytes) });
      toast.success('PDF document exported successfully');

      // Track export analytics
      await Analytics.trackButtonClick('export_pdf', 'meeting_details');
    } catch (error) {
      console.error('Failed to export PDF document:', error);
      toast.error('Failed to export PDF document');
    }
  }, [meeting, meetingTitle, aiSummary, blockNoteSummaryRef]);

  // Export summary to Outline
  const handleExportToOutline = useCallback(async () => {
    const config = loadOutlineConfig();
    if (!config.url || !config.apiKey || !config.collectionId) {
      toast.error('Outline is not configured. Go to Settings → Integrations to set it up.');
      return;
    }

    let summaryMarkdown = '';

    if (blockNoteSummaryRef.current?.getMarkdown) {
      summaryMarkdown = await blockNoteSummaryRef.current.getMarkdown();
    }
    if (!summaryMarkdown && aiSummary && 'markdown' in aiSummary) {
      summaryMarkdown = (aiSummary as any).markdown || '';
    }
    if (!summaryMarkdown && aiSummary) {
      summaryMarkdown = Object.entries(aiSummary)
        .filter(([key]) => key !== 'markdown' && key !== 'summary_json' && key !== '_section_order' && key !== 'MeetingName')
        .map(([, section]) => {
          if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
            return `## ${section.title}\n\n` + (section as any).blocks.map((b: any) => `- ${b.content}`).join('\n');
          }
          return '';
        })
        .filter(Boolean)
        .join('\n\n');
    }

    if (!summaryMarkdown.trim()) {
      toast.error('No summary content available to export');
      return;
    }

    const title = meetingTitle || 'Meeting Summary';
    const dateStr = new Date(meeting.created_at).toLocaleDateString('en-US', {
      year: 'numeric', month: 'long', day: 'numeric',
    });
    const fullMarkdown = `# ${title}\n\n**Date:** ${dateStr}\n\n---\n\n${summaryMarkdown}`;

    const loadingToastId = toast.loading('Exporting to Outline…');
    try {
      const docUrl = await exportToOutline(config, title, fullMarkdown);
      toast.dismiss(loadingToastId);
      toast.success('Exported to Outline', {
        description: 'Document created successfully.',
        action: docUrl ? { label: 'Open', onClick: () => invokeTauri('open_url', { url: docUrl }) } : undefined,
      });
      await Analytics.trackButtonClick('export_outline', 'meeting_details');
    } catch (error: any) {
      toast.dismiss(loadingToastId);
      console.error('Failed to export to Outline:', error);
      toast.error(`Failed to export to Outline: ${error?.message ?? error}`);
    }
  }, [meeting, meetingTitle, aiSummary, blockNoteSummaryRef]);

  return {
    handleCopyTranscript,
    handleCopySummary,
    handleExportPdf,
    handleExportToOutline,
  };
}
