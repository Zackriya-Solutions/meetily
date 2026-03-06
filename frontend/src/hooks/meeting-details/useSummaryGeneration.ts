import { useState, useCallback, useEffect } from 'react';
import { Transcript, Summary } from '@/types';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { toast } from 'sonner';
import { authFetch } from '@/lib/api';
import Analytics from '@/lib/analytics';

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';
type NotesGenerationInfo = {
  transcript_source?: string | null;
  audio_used?: boolean | null;
  agenda_used?: boolean | null;
  prompt_version?: string | null;
  diarized_available?: boolean;
  recommend_regenerate_with_diarized?: boolean;
};

interface UseSummaryGenerationProps {
  meeting: any;
  transcripts: Transcript[];
  modelConfig: ModelConfig;
  isModelConfigLoading: boolean;
  selectedTemplate: string;
  onMeetingUpdated?: () => Promise<void>;
  updateMeetingTitle: (title: string) => void;
  setAiSummary: (summary: Summary | null) => void;
  initialNotesGenerationInfo?: NotesGenerationInfo | null;
}

export function useSummaryGeneration({
  meeting,
  transcripts,
  modelConfig,
  isModelConfigLoading,
  selectedTemplate,
  onMeetingUpdated,
  updateMeetingTitle,
  setAiSummary,
  initialNotesGenerationInfo,
}: UseSummaryGenerationProps) {
  const [summaryStatus, setSummaryStatus] = useState<SummaryStatus>('idle');
  const [summaryError, setSummaryError] = useState<string | null>(null);
  const [notesGenerationInfo, setNotesGenerationInfo] = useState<NotesGenerationInfo | null>(
    initialNotesGenerationInfo || null
  );

  const { startSummaryPolling } = useSidebar();

  useEffect(() => {
    if (initialNotesGenerationInfo) {
      setNotesGenerationInfo(initialNotesGenerationInfo);
    }
  }, [initialNotesGenerationInfo]);

  // Helper to get status message
  const getSummaryStatusMessage = useCallback((status: SummaryStatus) => {
    switch (status) {
      case 'processing':
        return 'Processing transcript...';
      case 'summarizing':
        return 'Generating summary...';
      case 'regenerating':
        return 'Regenerating summary...';
      case 'completed':
        return 'Summary completed';
      case 'error':
        return 'Error generating summary';
      default:
        return '';
    }
  }, []);

  // Unified summary processing logic
  const processSummary = useCallback(async ({
    customPrompt = '',
    isRegeneration = false,
    preferDiarizedTranscript = true,
  }: {
    customPrompt?: string;
    isRegeneration?: boolean;
    preferDiarizedTranscript?: boolean;
  }) => {
      setSummaryStatus(isRegeneration ? 'regenerating' : 'processing');
      setSummaryError(null);

      try {
        console.log('Generating notes with Gemini using template:', selectedTemplate);

        // Use the new /meetings/{id}/generate-notes endpoint which uses Gemini
        const response = await authFetch(`/meetings/${meeting.id}/generate-notes`, {
          method: 'POST',
          // headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            meeting_id: meeting.id,
            template_id: selectedTemplate,
            model: 'gemini',
            model_name: 'gemini-2.5-pro',
            custom_context: customPrompt || '',  // Add context from user input
            prefer_diarized_transcript: preferDiarizedTranscript,
          })
        });

        if (!response.ok) {
          const errorData = await response.json().catch(() => ({}));
          throw new Error(errorData.detail || 'Failed to start notes generation');
        }
        const result = await response.json();

      // The new endpoint returns immediately with status: "processing"
      // We need to poll for the result using the meeting_id
      console.log('Notes generation started:', result);

      // Start global polling via context - use meeting.id since new endpoint doesn't return process_id
      startSummaryPolling(meeting.id, meeting.id, async (pollingResult) => {
        console.log('Summary status:', pollingResult);

        // Handle errors
        if (pollingResult.status === 'error' || pollingResult.status === 'failed') {
          console.error('Backend returned error:', pollingResult.error);
          const errorMessage = pollingResult.error || `Summary ${isRegeneration ? 'regeneration' : 'generation'} failed`;
          setSummaryError(errorMessage);
          setSummaryStatus('error');

          toast.error(`Failed to ${isRegeneration ? 'regenerate' : 'generate'} summary`, {
            description: errorMessage.includes('Connection refused')
              ? 'Could not connect to LLM service. Please ensure Ollama or your configured LLM provider is running.'
              : errorMessage,
          });

          await Analytics.trackSummaryGenerationCompleted(
            modelConfig.provider,
            modelConfig.model,
            false,
            undefined,
            errorMessage
          );
          return;
        }

        // Handle successful completion
        if (pollingResult.status === 'completed' && pollingResult.data) {
          console.log('✅ Summary generation completed:', pollingResult.data);
          if (pollingResult.notes_generation) {
            setNotesGenerationInfo(pollingResult.notes_generation);
          }

          // Update meeting title if available
          const meetingName = pollingResult.data.MeetingName || pollingResult.meetingName;
          if (meetingName) {
            updateMeetingTitle(meetingName);
          }

          // Check if backend returned markdown format (new flow)
          if (pollingResult.data.markdown) {
            console.log('✅ GENERATE SUMMARY: Received markdown format from backend');
            setAiSummary({ markdown: pollingResult.data.markdown } as any);
            setSummaryStatus('completed');

            if (meetingName && onMeetingUpdated) {
              await onMeetingUpdated();
            }

            await Analytics.trackSummaryGenerationCompleted(
              modelConfig.provider,
              modelConfig.model,
              true
            );

            if (isRegeneration && typeof window !== 'undefined') {
              const evt = new CustomEvent('show-share-dialog', { detail: { meetingId: meeting.id }});
              window.dispatchEvent(evt);
            }

            return;
          }

          // Legacy format handling
          const summarySections = Object.entries(pollingResult.data).filter(([key]) => key !== 'MeetingName');
          // Improved empty check to handle MeetingNotes structure
          const allEmpty = summarySections.every(([key, section]) => {
            if (key === 'MeetingNotes') {
              const notes = section as any;
              return !notes.sections || notes.sections.length === 0 || 
                     notes.sections.every((s: any) => !s.blocks || s.blocks.length === 0);
            }
            return !(section as any).blocks || (section as any).blocks.length === 0;
          });

          if (allEmpty) {
            console.error('Summary completed but all sections empty', pollingResult.data);
            setSummaryError('Summary generation completed but returned empty content.');
            setSummaryStatus('error');

            await Analytics.trackSummaryGenerationCompleted(
              modelConfig.provider,
              modelConfig.model,
              false,
              undefined,
              'Empty summary generated'
            );
            return;
          }

          // Remove MeetingName from data before formatting
          const { MeetingName, ...summaryData } = pollingResult.data;

          // Format legacy summary data
          const formattedSummary: Summary = {};
          const sectionKeys = pollingResult.data._section_order || Object.keys(summaryData);

          for (const key of sectionKeys) {
            try {
              const section = summaryData[key];
              
              // Handle MeetingNotes specially by flattening its sections
              if (key === 'MeetingNotes' && section && typeof section === 'object' && 'sections' in section) {
                const notes = section as { sections: any[] };
                notes.sections.forEach((s: any, idx: number) => {
                  if (s && s.title && Array.isArray(s.blocks)) {
                    // Use a unique key for each flattened section
                    const flattenKey = `notes_${idx}_${s.title.replace(/\s+/g, '_').toLowerCase()}`;
                    formattedSummary[flattenKey] = {
                      title: s.title,
                      blocks: s.blocks.map((block: any) => ({
                        ...block,
                        color: 'default',
                        content: block?.content?.trim() || ''
                      }))
                    };
                  }
                });
                continue;
              }

              if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
                const typedSection = section as { title?: string; blocks?: any[] };

                if (Array.isArray(typedSection.blocks)) {
                  formattedSummary[key] = {
                    title: typedSection.title || key,
                    blocks: typedSection.blocks.map((block: any) => ({
                      ...block,
                      color: 'default',
                      content: block?.content?.trim() || ''
                    }))
                  };
                } else {
                  formattedSummary[key] = {
                    title: typedSection.title || key,
                    blocks: []
                  };
                }
              }
            } catch (error) {
              console.warn(`Error processing section ${key}:`, error);
            }
          }

          setAiSummary(formattedSummary);
          setSummaryStatus('completed');

          await Analytics.trackSummaryGenerationCompleted(
            modelConfig.provider,
            modelConfig.model,
            true
          );

          if (meetingName && onMeetingUpdated) {
            await onMeetingUpdated();
          }

          if (isRegeneration && typeof window !== 'undefined') {
            const evt = new CustomEvent('show-share-dialog', { detail: { meetingId: meeting.id }});
            window.dispatchEvent(evt);
          }
        }
      });
    } catch (error) {
      console.error(`Failed to ${isRegeneration ? 'regenerate' : 'generate'} summary:`, error);
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      setSummaryError(errorMessage);
      setSummaryStatus('error');
      if (isRegeneration) {
        setAiSummary(null);
      }

      toast.error(`Failed to ${isRegeneration ? 'regenerate' : 'generate'} summary`, {
        description: errorMessage,
      });

      await Analytics.trackSummaryGenerationCompleted(
        modelConfig.provider,
        modelConfig.model,
        false,
        undefined,
        errorMessage
      );
    }
  }, [
    meeting.id,
    meeting.created_at,
    modelConfig,
    selectedTemplate,
    startSummaryPolling,
    setAiSummary,
    updateMeetingTitle,
    onMeetingUpdated,
  ]);

  // Public API: Generate summary from backend-selected transcript source.
  const handleGenerateSummary = useCallback(async (customPrompt: string = '') => {
    // Check if model config is still loading
    if (isModelConfigLoading) {
      console.log('⏳ Model configuration is still loading, please wait...');
      toast.info('Loading model configuration, please wait...');
      return;
    }

    if (!transcripts.length) {
      const error_msg = 'No transcripts available for summary';
      console.log(error_msg);
      toast.error(error_msg);
      return;
    }

    // Always use Gemini for notes generation (best quality)
    const notesProvider = 'gemini';
    const notesModel = 'gemini-2.5-pro';
    
    console.log('🚀 Starting notes generation with Gemini:', {
      provider: notesProvider,
      model: notesModel,
      template: selectedTemplate
    });

    await processSummary({ customPrompt, preferDiarizedTranscript: true });
  }, [transcripts, processSummary, isModelConfigLoading, selectedTemplate]);

  // Public API: Regenerate summary (same source preference as initial generation)
  const handleRegenerateSummary = useCallback(async () => {
    await processSummary({
      isRegeneration: true,
      preferDiarizedTranscript: true,
    });
  }, [processSummary]);

  const handleRegenerateWithDiarized = useCallback(async () => {
    await processSummary({
      isRegeneration: true,
      preferDiarizedTranscript: true,
    });
  }, [processSummary]);

  return {
    summaryStatus,
    summaryError,
    handleGenerateSummary,
    handleRegenerateSummary,
    handleRegenerateWithDiarized,
    notesGenerationInfo,
    getSummaryStatusMessage,
  };
}
