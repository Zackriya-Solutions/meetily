"use client"
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { useState, useEffect, useCallback, Suspense } from "react";
import PageContent from "./page-content";
import { useRouter, useSearchParams } from "next/navigation";
import Analytics from "@/lib/analytics";
import { invoke } from "@tauri-apps/api/core";
import { LoaderIcon } from "lucide-react";
import { useConfig } from "@/contexts/ConfigContext";
import { usePaginatedTranscripts } from "@/hooks/usePaginatedTranscripts";
import type { ModelConfig } from "@/services/configService";
import type { MeetingDetails } from "@/types/meeting";
import {
  parseSummaryPayloadFromApiData,
  type SummaryPayload,
} from "@/contracts/summaryContract";
import { DEFAULT_BUILTIN_SUMMARY_MODEL } from "@/constants/modelDefaults";

interface SummaryApiResponse {
  status: string;
  data?: unknown;
  error?: string;
}

function MeetingDetailsContent() {
  const searchParams = useSearchParams();
  const meetingId = searchParams.get('id');
  const source = searchParams.get('source'); // Check if navigated from recording
  const { setCurrentMeeting, refetchMeetings, stopSummaryPolling } = useSidebar();
  const { isAutoSummary } = useConfig(); // Get auto-summary toggle state
  const router = useRouter();
  const [meetingDetails, setMeetingDetails] = useState<MeetingDetails | null>(null);
  const [meetingSummary, setMeetingSummary] = useState<SummaryPayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [shouldAutoGenerate, setShouldAutoGenerate] = useState<boolean>(false);
  const [hasCheckedAutoGen, setHasCheckedAutoGen] = useState<boolean>(false);

  // Use pagination hook for efficient transcript loading
  const {
    metadata,
    segments,
    transcripts,
    isLoading: isLoadingTranscripts,
    isLoadingMore,
    hasMore,
    totalCount,
    loadedCount,
    loadMore,
    refetch,
    error: transcriptError,
  } = usePaginatedTranscripts({ meetingId: meetingId || '' });

  // Check if any built-in summary model is available for the default local path.
  const getAvailableBuiltinSummaryModel = useCallback(async (): Promise<string | null> => {
    try {
      const model = await invoke<string | null>('builtin_ai_get_available_summary_model');
      console.log('Checked for available built-in summary model:', model);
      return model;
    } catch (error) {
      console.error('Failed to check built-in summary models:', error);
      return null;
    }
  }, []);

  // Set up auto-generation - respects DB as source of truth
  const setupAutoGeneration = useCallback(async () => {
    if (hasCheckedAutoGen) return; // Only check once

    // Only auto-generate if navigated from recording
    if (source !== 'recording') {
      console.log('Not from recording navigation, skipping auto-generation');
      setHasCheckedAutoGen(true);
      return;
    }

    // Respect user's auto-summary toggle preference
    if (!isAutoSummary) {
      console.log('Auto-summary is disabled in settings');
      setHasCheckedAutoGen(true);
      return;
    }

    try {
      // Check what's currently in database
      const currentConfig = await invoke<ModelConfig | null>('model_cfg_get');

      // If DB already has a model, use it (never override!)
      if (currentConfig && currentConfig.model) {
        console.log('Using existing model from DB:', currentConfig.model);
        setShouldAutoGenerate(true);
        setHasCheckedAutoGen(true);
        return;
      }

      // DB is empty - only fall back to the local built-in summary path.
      const availableBuiltinModel = await getAvailableBuiltinSummaryModel();

      if (availableBuiltinModel) {
        console.log('DB empty, using available built-in summary model:', availableBuiltinModel);

        await invoke('model_cfg_set', {
          provider: 'builtin-ai',
          model: availableBuiltinModel || DEFAULT_BUILTIN_SUMMARY_MODEL,
          whisperModel: 'large-v3',
          apiKey: null,
          ollamaEndpoint: null,
        });

        setShouldAutoGenerate(true);
      } else {
        console.log('No configured summary model and no downloaded built-in model available yet');
      }
    } catch (error) {
      console.error('Failed to setup auto-generation:', error);
    }

    setHasCheckedAutoGen(true);
  }, [hasCheckedAutoGen, getAvailableBuiltinSummaryModel, source, isAutoSummary]);

  // Sync meeting metadata from pagination hook to meeting details state
  useEffect(() => {
    if (metadata && (!meetingId || meetingId === 'intro-call')) {
      // If invalid meeting ID, don't sync
      return;
    }

    if (metadata) {
      console.log('Meeting metadata loaded:', metadata);

      // Build meeting details from metadata and paginated transcripts
      setMeetingDetails({
        id: metadata.id,
        title: metadata.title,
        created_at: metadata.created_at,
        updated_at: metadata.updated_at,
        transcripts: transcripts, // Paginated transcripts from hook
        folder_path: metadata.folder_path, // For retranscription feature
      });

      // Sync with sidebar context
      setCurrentMeeting({ id: metadata.id, title: metadata.title });
    }
  }, [metadata, transcripts, meetingId, setCurrentMeeting]);

  // Handle transcript loading errors
  useEffect(() => {
    if (transcriptError) {
      console.error('Error loading transcripts:', transcriptError);
      setError(transcriptError);
    }
  }, [transcriptError]);

  // Reset states when meetingId changes (prevent race conditions)
  useEffect(() => {
    setMeetingDetails(null);
    setMeetingSummary(null);
    setError(null);
    setIsLoading(true);
    // Reset auto-generation state to allow new meeting to be checked
    setHasCheckedAutoGen(false);
    setShouldAutoGenerate(false);
  }, [meetingId]);

  // Cleanup: Stop polling when navigating away from a meeting
  useEffect(() => {
    return () => {
      if (meetingId) {
        console.log('Cleaning up: Stopping summary polling for meeting:', meetingId);
        stopSummaryPolling(meetingId);
      }
    };
  }, [meetingId, stopSummaryPolling]);

  useEffect(() => {
    console.log('MeetingDetails useEffect triggered - meetingId:', meetingId);

    if (!meetingId || meetingId === 'intro-call') {
      console.warn('No valid meeting ID in URL - meetingId:', meetingId);
      setError("No meeting selected");
      setIsLoading(false);
      Analytics.trackPageView('meeting_details');
      return;
    }

    console.log('Valid meeting ID found, fetching details for:', meetingId);

    setMeetingDetails(null);
    setMeetingSummary(null);
    setError(null);
    setIsLoading(true);

    const fetchMeetingSummary = async () => {
      try {
        const summary = await invoke<SummaryApiResponse>('api_get_summary', {
          meetingId: meetingId,
        });

        console.log('FETCH SUMMARY: Raw response:', summary);

        // Check if the summary request failed with 404 or error status, or if no summary exists yet (idle)
        // Note: 'cancelled' and 'failed' statuses can still have data if backup was restored
        if (summary.status === 'idle' || (!summary.data && summary.status === 'error')) {
          console.warn('Meeting summary not found or no summary generated yet:', summary.error || 'idle');
          setMeetingSummary(null);
          return;
        }

        const parsed = parseSummaryPayloadFromApiData(summary.data);
        if (!parsed.ok) {
          console.error('FETCH SUMMARY: Invalid summary contract payload', parsed.error);
          setMeetingSummary(null);
          return;
        }

        console.log('FETCH SUMMARY: Parsed v0.1.0 payload:', parsed.data.format);
        setMeetingSummary(parsed.data);
      } catch (error) {
        console.error('FETCH SUMMARY: Error fetching meeting summary:', error);
        // Don't set error state for summary fetch failure, set to null to show generate button
        setMeetingSummary(null);
      }
    };

    const loadData = async () => {
      try {
        await fetchMeetingSummary();
      } finally {
        setIsLoading(false);
      }
    };

    loadData();
  }, [meetingId]);

  // Auto-generation check: runs when meeting is loaded with no summary
  useEffect(() => {
    const checkAutoGen = async () => {
      // Only auto-generate if:
      // 1. We have meeting details
      // 2. No summary exists
      // 3. Meeting has transcripts
      // 4. Haven't checked yet
      if (
        meetingDetails &&
        meetingSummary === null &&
        meetingDetails.transcripts &&
        meetingDetails.transcripts.length > 0 &&
        !hasCheckedAutoGen
      ) {
        console.log('No summary found, checking for auto-generation...');
        await setupAutoGeneration();
      }
    };

    checkAutoGen();
  }, [meetingDetails, meetingSummary, hasCheckedAutoGen, setupAutoGeneration]);

  if (error) {
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="text-center">
          <p className="text-red-500 mb-4">{error}</p>
          <button
            onClick={() => router.push('/')}
            className="px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
          >
            Go Back
          </button>
        </div>
      </div>
    );
  }

  // Show loading spinner while initial data loads
  if ((isLoading || isLoadingTranscripts) || !meetingDetails) {
    return <div className="flex items-center justify-center h-screen">
      <LoaderIcon className="animate-spin size-6 " />
    </div>;
  }

  return <PageContent
    meeting={meetingDetails}
    summaryData={meetingSummary}
    shouldAutoGenerate={shouldAutoGenerate}
    onAutoGenerateComplete={() => setShouldAutoGenerate(false)}
    onMeetingUpdated={async () => {
      await refetch();
      await refetchMeetings();
    }}
    onRefetchTranscripts={refetch}
    // Pagination props for efficient transcript loading
    segments={segments}
    hasMore={hasMore}
    isLoadingMore={isLoadingMore}
    totalCount={totalCount}
    loadedCount={loadedCount}
    onLoadMore={loadMore}
  />;
}

export default function MeetingDetails() {
  return (
    <Suspense fallback={
      <div className="flex items-center justify-center h-screen">
        <LoaderIcon className="animate-spin size-6" />
      </div>
    }>
      <MeetingDetailsContent />
    </Suspense>
  );
}


