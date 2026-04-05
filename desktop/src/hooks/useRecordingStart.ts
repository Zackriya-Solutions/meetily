import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { recordingService } from '@/services/recordingService';
import Analytics from '@/lib/analytics';
import { showRecordingNotification } from '@/lib/recordingNotification';
import { toast } from 'sonner';

interface ParakeetModelInfo {
  status?: unknown;
}

interface UseRecordingStartReturn {
  handleRecordingStart: () => Promise<void>;
  isAutoStarting: boolean;
}

/**
 * Custom hook for managing recording start lifecycle.
 * Handles both manual start (button click) and auto-start (from sidebar navigation).
 *
 * Features:
 * - Meeting title generation (format: Meeting DD_MM_YY_HH_MM_SS)
 * - Transcript clearing on start
 * - Analytics tracking
 * - Recording notification display
 * - Auto-start from sidebar via sessionStorage flag
 */
export function useRecordingStart(
  isRecording: boolean,
  setIsRecording: (value: boolean) => void,
  showModal?: (name: 'modelSelector', message?: string) => void
): UseRecordingStartReturn {
  const [isAutoStarting, setIsAutoStarting] = useState(false);

  const { clearTranscripts, setMeetingTitle } = useTranscripts();
  const { setIsMeetingActive } = useSidebar();
  const { selectedDevices } = useConfig();
  const { setStatus } = useRecordingState();

  // Generate meeting title with timestamp
  const generateMeetingTitle = useCallback(() => {
    const now = new Date();
    const day = String(now.getDate()).padStart(2, '0');
    const month = String(now.getMonth() + 1).padStart(2, '0');
    const year = String(now.getFullYear()).slice(-2);
    const hours = String(now.getHours()).padStart(2, '0');
    const minutes = String(now.getMinutes()).padStart(2, '0');
    const seconds = String(now.getSeconds()).padStart(2, '0');
    return `Meeting ${day}_${month}_${year}_${hours}_${minutes}_${seconds}`;
  }, []);

  // Check if Parakeet transcription model is ready
  const checkParakeetReady = useCallback(async (): Promise<boolean> => {
    try {
      await invoke('parakeet_init');
      const hasModels = await invoke<boolean>('parakeet_has_available_models');
      return hasModels;
    } catch (error) {
      console.error('Failed to check Parakeet status:', error);
      return false;
    }
  }, []);

  // Check if any model is currently downloading
  const checkIfModelDownloading = useCallback(async (): Promise<boolean> => {
    try {
      const models = await invoke<ParakeetModelInfo[]>('parakeet_get_available_models');
      const isDownloading = models.some(m =>
        m.status && (
          typeof m.status === 'object'
            ? 'Downloading' in m.status
            : m.status === 'Downloading'
        )
      );
      return isDownloading;
    } catch (error) {
      console.error('Failed to check model download status:', error);
      return false; // Default to not downloading (will show error + modal)
    }
  }, []);

  const ensureRecordingModelReady = useCallback(async (analyticsSource: string): Promise<boolean> => {
    const parakeetReady = await checkParakeetReady();
    if (parakeetReady) {
      return true;
    }

    const isDownloading = await checkIfModelDownloading();
    if (isDownloading) {
      toast.info('Model download in progress', {
        description: 'Please wait for the transcription model to finish downloading before recording.',
        duration: 5000,
      });
      Analytics.trackButtonClick('start_recording_blocked_downloading', analyticsSource);
    } else {
      toast.error('Transcription model not ready', {
        description: 'Please download a transcription model before recording.',
        duration: 5000,
      });
      showModal?.('modelSelector', 'Transcription model setup required');
      Analytics.trackButtonClick('start_recording_blocked_missing', analyticsSource);
    }

    setStatus(RecordingStatus.IDLE);
    return false;
  }, [checkIfModelDownloading, checkParakeetReady, setStatus, showModal]);

  const runRecordingStart = useCallback(async (analyticsSource: string) => {
    const modelReady = await ensureRecordingModelReady(analyticsSource);
    if (!modelReady) {
      return false;
    }

    const generatedMeetingTitle = generateMeetingTitle();
    setMeetingTitle(generatedMeetingTitle);
    setStatus(RecordingStatus.STARTING, 'Initializing recording...');

    await recordingService.startRecordingWithDevices(
      selectedDevices?.micDevice || null,
      selectedDevices?.systemDevice || null,
      generatedMeetingTitle
    );

    setIsRecording(true);
    clearTranscripts();
    setIsMeetingActive(true);
    Analytics.trackButtonClick('start_recording', analyticsSource);
    await showRecordingNotification();
    return true;
  }, [
    clearTranscripts,
    ensureRecordingModelReady,
    generateMeetingTitle,
    selectedDevices,
    setIsMeetingActive,
    setIsRecording,
    setMeetingTitle,
    setStatus,
  ]);

  const handleStartFailure = useCallback((error: unknown, analyticsSource: string, fallbackMessage: string) => {
    console.error(fallbackMessage, error);
    setStatus(RecordingStatus.ERROR, error instanceof Error ? error.message : fallbackMessage);
    setIsRecording(false);
    Analytics.trackButtonClick('start_recording_error', analyticsSource);
  }, [setIsRecording, setStatus]);

  const handleRecordingStart = useCallback(async () => {
    try {
      await runRecordingStart('home_page');
    } catch (error) {
      handleStartFailure(error, 'home_page', 'Failed to start recording');
      throw error;
    }
  }, [handleStartFailure, runRecordingStart]);

  // Check for autoStartRecording flag and start recording automatically
  useEffect(() => {
    const checkAutoStartRecording = async () => {
      if (typeof window !== 'undefined') {
        const shouldAutoStart = sessionStorage.getItem('autoStartRecording');
        if (shouldAutoStart === 'true' && !isRecording && !isAutoStarting) {
          console.log('Auto-starting recording from navigation...');
          setIsAutoStarting(true);
          sessionStorage.removeItem('autoStartRecording'); // Clear the flag

          try {
            await runRecordingStart('sidebar_auto');
          } catch (error) {
            handleStartFailure(error, 'sidebar_auto', 'Failed to auto-start recording');
            toast.error('Failed to start recording', {
              description: error instanceof Error ? error.message : 'Unknown error occurred',
            });
          } finally {
            setIsAutoStarting(false);
          }
        }
      }
    };

    checkAutoStartRecording();
  }, [
    isRecording,
    isAutoStarting,
    selectedDevices,
    generateMeetingTitle,
    setMeetingTitle,
    setIsRecording,
    clearTranscripts,
    setIsMeetingActive,
    checkParakeetReady,
    checkIfModelDownloading,
    showModal,
    setStatus,
  ]);

  // Listen for direct recording trigger from sidebar when already on home page
  useEffect(() => {
    const handleDirectStart = async () => {
      if (isRecording || isAutoStarting) {
        console.log('Recording already in progress, ignoring direct start event');
        return;
      }

      console.log('Direct start from sidebar - checking Parakeet model status');
      setIsAutoStarting(true);

      try {
        await runRecordingStart('sidebar_direct');
      } catch (error) {
        handleStartFailure(error, 'sidebar_direct', 'Failed to start recording from sidebar');
        toast.error('Failed to start recording', {
          description: error instanceof Error ? error.message : 'Unknown error occurred',
        });
      } finally {
        setIsAutoStarting(false);
      }
    };

    window.addEventListener('start-recording-from-sidebar', handleDirectStart);

    return () => {
      window.removeEventListener('start-recording-from-sidebar', handleDirectStart);
    };
  }, [
    isRecording,
    isAutoStarting,
    runRecordingStart,
    handleStartFailure,
  ]);

  return {
    handleRecordingStart,
    isAutoStarting,
  };
}
