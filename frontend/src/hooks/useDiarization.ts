
import { useState, useEffect, useCallback, useMemo } from 'react';
import { authFetch } from '@/lib/api';

export interface DiarizationStatus {
  meeting_id: string;
  status: 'pending' | 'processing' | 'completed' | 'failed' | 'not_recorded' | 'stopped';
  speaker_count?: number;
  provider?: string;
  error?: string;
  completed_at?: string;
}

export interface SpeakerMapping {
  label: string;
  display_name: string;
  color?: string;
}

export interface DiarizationProgress {
  meeting_id: string;
  total_chunks: number;
  processed_chunks: number;
  completed_chunks: number;
  failed_chunks: number;
  processing_chunks: number;
  pending_chunks: number;
  percent_complete: number;
}

export function useDiarization(meetingId: string) {
  const [status, setStatus] = useState<DiarizationStatus | null>(null);
  const [speakers, setSpeakers] = useState<SpeakerMapping[]>([]);
  const [isDiarizing, setIsDiarizing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<DiarizationProgress | null>(null);
  const [processingStartedAtMs, setProcessingStartedAtMs] = useState<number | null>(null);

  const toUserFriendlyError = useCallback((raw: string | null | undefined): string => {
    const message = (raw || '').trim();
    if (!message) return 'Diarization failed. Please try again.';

    const lower = message.toLowerCase();
    const isGroqRateLimit =
      lower.includes('rate limit') &&
      lower.includes('whisper-large-v3') &&
      lower.includes('please try again in');

    if (isGroqRateLimit) {
      const retryMatch = message.match(/Please try again in ([^.,'"}]+)/i);
      const retryText = retryMatch?.[1]?.trim();
      if (retryText) {
        return `Transcription provider rate limit reached. Please retry in about ${retryText}.`;
      }
      return 'Transcription provider rate limit reached. Please try again in a minute.';
    }

    if (lower.includes('operator is not unique')) {
      return 'Temporary backend database type error occurred while saving diarization progress. Please retry diarization.';
    }

    if (lower.includes('request_too_large') || lower.includes('payload too large')) {
      return 'Audio is too large for one transcription request. Please retry; chunked fallback will be used when available.';
    }

    return 'Diarization failed. Please retry.';
  }, []);

  const fetchStatus = useCallback(async () => {
    if (!meetingId) return;
    try {
      const response = await authFetch(`/meetings/${meetingId}/diarization-status`);
      if (response.ok) {
        const data = await response.json();
        setStatus(data);
      }
    } catch (err) {
      console.error('Failed to fetch diarization status:', err);
    }
  }, [meetingId]);

  const fetchSpeakers = useCallback(async () => {
    if (!meetingId) return;
    try {
      const response = await authFetch(`/meetings/${meetingId}/speakers`);
      if (response.ok) {
        const data = await response.json();
        setSpeakers(data.speakers || []);
      }
    } catch (err) {
      console.error('Failed to fetch speakers:', err);
    }
  }, [meetingId]);

  const fetchProgress = useCallback(async () => {
    if (!meetingId) return;
    try {
      const response = await authFetch(`/meetings/${meetingId}/diarization-progress`);
      if (response.ok) {
        const data = await response.json();
        setProgress(data);
      }
    } catch (err) {
      // Progress endpoint is optional in older backend versions; ignore fetch errors.
    }
  }, [meetingId]);

  // Initial load
  useEffect(() => {
    fetchStatus();
    fetchSpeakers();
    fetchProgress();
  }, [meetingId, fetchStatus, fetchSpeakers, fetchProgress]);

  useEffect(() => {
    if (status?.status === 'processing') {
      if (!processingStartedAtMs) {
        setProcessingStartedAtMs(Date.now());
      }
      // While processing, clear stale error from previous run.
      if (error) setError(null);
    } else {
      setProcessingStartedAtMs(null);
      if (status?.status === 'completed' || status?.status === 'failed' || status?.status === 'stopped') {
        setProgress(null);
      }
    }
  }, [status?.status, processingStartedAtMs, error]);

  // Surface backend-reported status errors in UI toast layer.
  useEffect(() => {
    if (status?.status === 'failed') {
      setError(toUserFriendlyError(status.error));
    } else if (status?.status === 'completed' || status?.status === 'stopped') {
      setError(null);
    }
  }, [status?.status, status?.error, toUserFriendlyError]);

  // Separate polling effect
  useEffect(() => {
    let interval: NodeJS.Timeout;
    if (status?.status === 'processing') {
       interval = setInterval(() => {
        fetchStatus();
        fetchProgress();
       }, 5000);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [status?.status, fetchStatus, fetchProgress]);

  const waitEstimateText = useMemo(() => {
    if (status?.status !== 'processing') return null;

    if (!progress || progress.total_chunks <= 0 || !processingStartedAtMs) {
      return 'Usually takes about 2-4 minutes';
    }

    if (progress.processed_chunks <= 0) {
      return 'Starting speaker identification...';
    }

    const elapsedSec = Math.max(1, (Date.now() - processingStartedAtMs) / 1000);
    const secPerChunk = elapsedSec / Math.max(1, progress.processed_chunks);
    const remainingChunks = Math.max(0, progress.total_chunks - progress.processed_chunks);
    const remainingSec = Math.round(secPerChunk * remainingChunks);

    if (remainingSec <= 60) return `About ${remainingSec}s remaining`;
    const remainingMin = Math.ceil(remainingSec / 60);
    return `About ${remainingMin} min remaining`;
  }, [status?.status, progress, processingStartedAtMs]);

  const triggerDiarization = async () => {
    setIsDiarizing(true);
    setError(null);
    try {
      const response = await authFetch(`/meetings/${meetingId}/diarize`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider: 'deepgram' }) // Default to deepgram
      });

      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.detail || 'Diarization failed');
      }

      await fetchStatus();
      await fetchProgress();
    } catch (err: any) {
      setError(toUserFriendlyError(err?.message));
    } finally {
      setIsDiarizing(false);
    }
  };

  const stopDiarization = async () => {
    try {
      const response = await authFetch(`/meetings/${meetingId}/diarize/stop`, {
        method: 'POST',
      });

      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.detail || 'Failed to stop diarization');
      }

      await fetchStatus();
      await fetchProgress();
    } catch (err: any) {
      console.error('Failed to stop diarization:', err);
      setError(toUserFriendlyError(err?.message));
    }
  };

  const renameSpeaker = async (label: string, newName: string) => {
    try {
      const response = await authFetch(`/meetings/${meetingId}/speakers/${label}/rename`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ display_name: newName })
      });

      if (!response.ok) {
        throw new Error('Failed to rename speaker');
      }

      // Optimistic update
      setSpeakers(prev => prev.map(s => 
        s.label === label ? { ...s, display_name: newName } : s
      ));
      
      return true;
    } catch (err) {
      console.error(err);
      return false;
    }
  };

  return {
    status,
    speakers,
    isDiarizing,
    error,
    progress,
    waitEstimateText,
    triggerDiarization,
    stopDiarization,
    renameSpeaker,
    refreshSpeakers: fetchSpeakers
  };
}
