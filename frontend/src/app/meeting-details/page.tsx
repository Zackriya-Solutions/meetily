"use client"
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { useState, useEffect, useCallback, Suspense } from "react";
import { Transcript, Summary } from "@/types";
import PageContent from "./page-content";
import { useRouter, useSearchParams } from "next/navigation";
import Analytics from "@/lib/analytics";
import { LoaderIcon } from "lucide-react";
import { authFetch } from "@/lib/api";

interface MeetingDetailsResponse {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  transcripts: Transcript[];
}
interface NotesGenerationInfo {
  transcript_source?: string | null;
  audio_used?: boolean | null;
  agenda_used?: boolean | null;
  prompt_version?: string | null;
  diarized_available?: boolean;
  recommend_regenerate_with_diarized?: boolean;
}

function MeetingDetailsContent() {
  const searchParams = useSearchParams();
  const meetingId = searchParams.get('id');
  const { setCurrentMeeting, refetchMeetings, serverAddress, refetchSharedNotes } = useSidebar();
  const router = useRouter();
  const [meetingDetails, setMeetingDetails] = useState<MeetingDetailsResponse | null>(null);
  const [meetingSummary, setMeetingSummary] = useState<Summary | null>(null);
  const [notesGenerationInfo, setNotesGenerationInfo] = useState<NotesGenerationInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [shouldAutoGenerate, setShouldAutoGenerate] = useState<boolean>(false);
  const [hasCheckedAutoGen, setHasCheckedAutoGen] = useState<boolean>(false);

  // Check if gemma3:1b model is available in Ollama
  const checkForGemmaModel = useCallback(async (): Promise<boolean> => {
    try {
      const response = await fetch('http://localhost:11434/api/tags');
      if (!response.ok) throw new Error('Failed to fetch models');
      const data = await response.json();
      const models = data.models || [];
      const hasGemma = models.some((m: any) => m.name === 'gemma3:1b');
      console.log('🔍 Checked for gemma3:1b:', hasGemma);
      return hasGemma;
    } catch (error) {
      console.error('❌ Failed to check Ollama models:', error);
      return false;
    }
  }, []);

  // Set up auto-generation - respects DB as source of truth
  const setupAutoGeneration = useCallback(async () => {
    if (hasCheckedAutoGen) return; // Only check once

    try {
      if (!serverAddress) return;
      // ✅ STEP 1: Check what's currently in database
      const configResponse = await authFetch('/get-model-config');
      const currentConfig = await configResponse.json();

      // ✅ STEP 2: If DB already has a model, use it (never override!)
      if (currentConfig && currentConfig.model) {
        console.log('✅ Using existing model from DB:', currentConfig.model);
        setShouldAutoGenerate(true);
        setHasCheckedAutoGen(true);
        return;
      }

      // ✅ STEP 3: DB is empty - apply Gemini 1.5 Flash as default
      console.log('💾 DB empty, using Gemini 1.5 Flash as initial default');

      await authFetch('/save-model-config', {
        method: 'POST',
        body: JSON.stringify({
          provider: 'gemini',
          model: 'gemini-2.5-pro',
          whisperModel: 'large-v3',
          apiKey: null // Will use env var on backend
        })
      });

      setShouldAutoGenerate(true);
    } catch (error) {
      console.error('❌ Failed to setup auto-generation:', error);
    }

    setHasCheckedAutoGen(true);
  }, [hasCheckedAutoGen, serverAddress]);

  // Extract fetchMeetingDetails so it can be called from child components
    const fetchMeetingDetails = useCallback(async () => {
    if (!meetingId || meetingId === 'intro-call' || !serverAddress) {
      return;
    }

    try {
      const isShared = searchParams.get('shared') === 'true';
      const url = isShared ? `/api/sharing/${meetingId}` : `/get-meeting/${meetingId}`;
      const response = await authFetch(url);
      
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      
      const data = await response.json();
      
      if (isShared) {
        // Shared response format: { meeting: {...}, summary: {...}, transcripts: [...] }
        if (data.meeting) {
          data.meeting.transcripts = data.transcripts || [];
          setMeetingDetails(data.meeting);
          setCurrentMeeting({ id: data.meeting.id, title: data.meeting.title });
        }
        
        // Also trigger the "viewed" endpoint in the background
        authFetch(`/api/sharing/${meetingId}/viewed`, { method: 'PATCH' })
          .then(() => {
            if (refetchSharedNotes) refetchSharedNotes();
          })
          .catch(err => console.error("Failed to mark shared note viewed", err));
          
      } else {
        // Standard response format
        console.log('Meeting details:', data);
        setMeetingDetails(data);
        setCurrentMeeting({ id: data.id, title: data.title });
      }
    } catch (error) {
      console.error('Error fetching meeting details:', error);
      setError("Failed to load meeting details");
    }
  }, [meetingId, setCurrentMeeting, serverAddress, searchParams]);

    const fetchMeetingSummary = useCallback(async () => {
    if (!meetingId || meetingId === 'intro-call' || !serverAddress) return;
    try {
      const isShared = searchParams.get('shared') === 'true';
      const url = isShared ? `/api/sharing/${meetingId}` : `/get-summary/${meetingId}`;
      const response = await authFetch(url);
      
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const rawResponse = await response.json();
      
      // Extract summary based on endpoint
      let summary = rawResponse;
      if (isShared) {
        // If shared, the summary is embedded, but it might not be formatted exactly like `/get-summary`
        // we'll try to map it to the expected structure
        if (!rawResponse.summary) {
           setMeetingSummary(null);
           return;
        }
        // Transform shared summary data into what the code below expects
        summary = {
           status: "completed",
           data: rawResponse.summary,
           notes_generation: null
        };
      }
      
      setNotesGenerationInfo(summary.notes_generation || null);
      console.log('🔍 FETCH SUMMARY: Received data:', summary);

      if (summary.status === 'error' || summary.error || summary.status === 'idle') {
        console.warn('⚠️ FETCH SUMMARY: Summary state is incomplete:', summary.status);
        setMeetingSummary(null);
        setNotesGenerationInfo(summary.notes_generation || null);
        return;
      }

      const summaryData = summary.data || {};
      let parsedData = summaryData;
      if (typeof summaryData === 'string') {
        try {
          parsedData = JSON.parse(summaryData);
        } catch (e) {
          console.warn('⚠️ FETCH SUMMARY: Failed to parse string data:', e);
          parsedData = {};
        }
      }

      // Check for modern formats first and preserve them
      if (parsedData.summary_json || parsedData.markdown) {
        console.log('✅ FETCH SUMMARY: Detected modern format (markdown/json)');
        setMeetingSummary(parsedData as any);
        return;
      }

      // Legacy format handling
      console.log('📝 FETCH SUMMARY: Falling back to legacy format processing');
      const { MeetingName, _section_order, ...restSummaryData } = parsedData;
      const formattedSummary: Summary = {};
      const sectionKeys = _section_order || Object.keys(restSummaryData);

      for (const key of sectionKeys) {
        try {
          const section = restSummaryData[key];

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
            formattedSummary[key] = {
              title: section.title || key,
              blocks: Array.isArray(section.blocks) ? section.blocks.map((block: any) => ({
                ...block,
                color: 'default',
                content: block?.content?.trim() || ''
              })) : []
            };
          }
        } catch (error) {
          console.warn(`Error processing section ${key}:`, error);
        }
      }
      setMeetingSummary(formattedSummary);
    } catch (error) {
      console.error('❌ FETCH SUMMARY Error:', error);
      setMeetingSummary(null);
      setNotesGenerationInfo(null);
    }
  }, [meetingId, serverAddress, searchParams]);

  const handleMeetingUpdated = useCallback(async () => {
    await fetchMeetingDetails();
    await fetchMeetingSummary();
    await refetchMeetings();
  }, [fetchMeetingDetails, fetchMeetingSummary, refetchMeetings]);

  // Reset states when meetingId changes
  useEffect(() => {
    setMeetingDetails(null);
    setMeetingSummary(null);
    setError(null);
    setIsLoading(true);
  }, [meetingId]);

  // Initial load
  useEffect(() => {
    if (!meetingId || meetingId === 'intro-call') {
      setError("No meeting selected");
      setIsLoading(false);
      return;
    }

    const loadData = async () => {
      if (!serverAddress) return;
      try {
        await Promise.all([
          fetchMeetingDetails(),
          fetchMeetingSummary()
        ]);
      } finally {
        setIsLoading(false);
      }
    };

    if (serverAddress) {
      loadData();
    }
  }, [meetingId, fetchMeetingDetails, fetchMeetingSummary, serverAddress]);

  // Poll for summary if missing
  useEffect(() => {
    if (meetingDetails && meetingSummary === null && (meetingDetails.transcripts?.length || 0) > 0) {
      const pollInterval = setInterval(async () => {
        try {
          if (!serverAddress) return;
          const response = await authFetch(`/get-summary/${meetingId}`);
          if (response.ok) {
            const summary = await response.json();
            if (summary.status === 'completed' && summary.data) {
              clearInterval(pollInterval);
              await fetchMeetingSummary();
            } else if (summary.status === 'error') {
              clearInterval(pollInterval);
            }
          }
        } catch (error) {
          console.error('Error polling summary:', error);
        }
      }, 5000);

      const timeout = setTimeout(() => {
        clearInterval(pollInterval);
      }, 180000);

      return () => {
        clearInterval(pollInterval);
        clearTimeout(timeout);
      };
    }
  }, [meetingDetails, meetingSummary, meetingId, serverAddress, fetchMeetingSummary]);

  if (error) {
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="text-center">
          <p className="text-red-500 mb-4">{error}</p>
          <button onClick={() => router.push('/')} className="px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600">
            Go Back
          </button>
        </div>
      </div>
    );
  }

  if (isLoading || !meetingDetails) {
    return <div className="flex items-center justify-center h-screen"><LoaderIcon className="animate-spin size-6" /></div>;
  }

  return <PageContent
    meeting={meetingDetails}
    summaryData={meetingSummary}
    notesGenerationInfo={notesGenerationInfo}
    shouldAutoGenerate={shouldAutoGenerate}
    onAutoGenerateComplete={() => setShouldAutoGenerate(false)}
    onMeetingUpdated={handleMeetingUpdated}
  />;
}

export default function MeetingDetails() {
  return (
    <Suspense fallback={<div className="flex items-center justify-center h-screen"><LoaderIcon className="animate-spin size-6" /></div>}>
      <MeetingDetailsContent />
    </Suspense>
  );
}
