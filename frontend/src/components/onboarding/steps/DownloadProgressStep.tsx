import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mic, Sparkles, Check, Loader2, Download, Users } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { toast } from 'sonner';
import { motion, AnimatePresence } from 'framer-motion';
import { ClearMinutesIcon } from '@/components/ClearMinutesLogo';

const PARAKEET_MODEL = 'parakeet-tdt-0.6b-v3-int8';

type DownloadStatus = 'waiting' | 'downloading' | 'completed' | 'error';

interface DownloadState {
  status: DownloadStatus;
  progress: number;
  downloadedMb: number;
  totalMb: number;
  speedMbps: number;
  error?: string;
}

export function DownloadProgressStep() {
  const {
    goNext,
    selectedSummaryModel,
    setSelectedSummaryModel,
    parakeetDownloaded,
    setParakeetDownloaded,
    summaryModelDownloaded,
    setSummaryModelDownloaded,
    startBackgroundDownloads,
    completeOnboarding,
  } = useOnboarding();

  const [recommendedModel, setRecommendedModel] = useState<string>('gemma3:1b');
  const [isMac, setIsMac] = useState(false);

  const [parakeetState, setParakeetState] = useState<DownloadState>({
    status: parakeetDownloaded ? 'completed' : 'waiting',
    progress: parakeetDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 670,
    speedMbps: 0,
  });

  const [gemmaState, setGemmaState] = useState<DownloadState>({
    status: summaryModelDownloaded ? 'completed' : 'waiting',
    progress: summaryModelDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 806,
    speedMbps: 0,
  });

  const [diarizationState, setDiarizationState] = useState<DownloadState>({
    status: 'waiting',
    progress: 0,
    downloadedMb: 0,
    totalMb: 217,
    speedMbps: 0,
  });

  const [isCompleting, setIsCompleting] = useState(false);
  const downloadStartedRef = useRef(false);
  const retryingRef = useRef(false);
  const retryingSummaryRef = useRef(false);
  const diarizationSubRef = useRef({ segmentation: false, embedding: false });

  // ── Retry handlers ────────────────────────────────────────────────────────

  const handleRetryDownload = async () => {
    if (retryingRef.current) return;
    retryingRef.current = true;
    setParakeetState(prev => ({ ...prev, status: 'waiting', error: undefined, progress: 0, downloadedMb: 0, speedMbps: 0 }));
    try {
      await invoke('parakeet_retry_download', { modelName: PARAKEET_MODEL });
    } catch (error) {
      setParakeetState(prev => ({ ...prev, status: 'error', error: error instanceof Error ? error.message : 'Retry failed' }));
      toast.error('Download retry failed', { description: 'Please check your connection and try again.' });
    } finally {
      setTimeout(() => { retryingRef.current = false; }, 2000);
    }
  };

  const handleRetrySummaryDownload = async () => {
    if (retryingSummaryRef.current) return;
    retryingSummaryRef.current = true;
    setGemmaState(prev => ({ ...prev, status: 'downloading', error: undefined, progress: 0, downloadedMb: 0, speedMbps: 0 }));
    try {
      await invoke('builtin_ai_download_model', { modelName: selectedSummaryModel || recommendedModel });
    } catch (error) {
      setGemmaState(prev => ({ ...prev, status: 'error', error: error instanceof Error ? error.message : 'Retry failed' }));
      toast.error('Summary model download retry failed', { description: 'Please check your connection and try again.' });
    } finally {
      setTimeout(() => { retryingSummaryRef.current = false; }, 2000);
    }
  };

  const handleRetryDiarizationDownload = async () => {
    setDiarizationState(prev => ({ ...prev, status: 'downloading', error: undefined, progress: 0, downloadedMb: 0 }));
    try {
      if (!diarizationSubRef.current.segmentation) {
        await invoke('diarization_download_model', { modelType: 'segmentation' });
      }
      if (!diarizationSubRef.current.embedding) {
        await invoke('diarization_download_model', { modelType: 'embedding' });
      }
    } catch (error) {
      setDiarizationState(prev => ({ ...prev, status: 'error', error: String(error) }));
    }
  };

  // ── Platform & model detection ─────────────────────────────────────────────

  useEffect(() => {
    const fetchRecommendation = async () => {
      try {
        const model = await invoke<string>('builtin_ai_get_recommended_model');
        setRecommendedModel(model);
        setSelectedSummaryModel(model);
      } catch (error) {
        console.error('Failed to get recommended model:', error);
      }
    };
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    fetchRecommendation();
    checkPlatform();
  }, []);

  // ── Start downloads on mount ───────────────────────────────────────────────

  useEffect(() => {
    if (downloadStartedRef.current) return;
    downloadStartedRef.current = true;
    startDownloads();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Parakeet event listeners ───────────────────────────────────────────────

  useEffect(() => {
    const unlistenProgress = listen<{
      modelName: string; progress: number; downloaded_mb?: number;
      total_mb?: number; speed_mbps?: number; status?: string;
    }>('parakeet-model-download-progress', (event) => {
      const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
      if (modelName === PARAKEET_MODEL) {
        setParakeetState(prev => ({
          ...prev,
          status: status === 'completed' ? 'completed' : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
        }));
        if (status === 'completed' || progress >= 100) setParakeetDownloaded(true);
      }
    });
    const unlistenComplete = listen<{ modelName: string }>('parakeet-model-download-complete', (event) => {
      if (event.payload.modelName === PARAKEET_MODEL) {
        setParakeetState(prev => ({ ...prev, status: 'completed', progress: 100 }));
        setParakeetDownloaded(true);
      }
    });
    const unlistenError = listen<{ modelName: string; error: string }>('parakeet-model-download-error', (event) => {
      if (event.payload.modelName === PARAKEET_MODEL) {
        setParakeetState(prev => ({ ...prev, status: 'error', error: event.payload.error }));
      }
    });
    return () => {
      unlistenProgress.then(fn => fn());
      unlistenComplete.then(fn => fn());
      unlistenError.then(fn => fn());
    };
  }, []);

  // ── Gemma event listeners ──────────────────────────────────────────────────

  useEffect(() => {
    const unlisten = listen<{
      model: string; progress: number; downloaded_mb?: number;
      total_mb?: number; speed_mbps?: number; status: string; error?: string;
    }>('builtin-ai-download-progress', (event) => {
      const { model, progress, downloaded_mb, total_mb, speed_mbps, status, error } = event.payload;
      if (model === selectedSummaryModel || model === 'gemma3:1b' || model === 'gemma3:4b') {
        setGemmaState(prev => ({
          ...prev,
          status: status === 'completed' ? 'completed' : status === 'error' ? 'error' : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
          error: status === 'error' ? error : undefined,
        }));
        if (status === 'completed' || progress >= 100) setSummaryModelDownloaded(true);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [selectedSummaryModel]);

  // ── Diarization event listeners ────────────────────────────────────────────

  useEffect(() => {
    invoke<{ segmentation_available: boolean; embedding_available: boolean; both_available: boolean }>(
      'diarization_check_models'
    ).then(status => {
      if (status.both_available) {
        diarizationSubRef.current = { segmentation: true, embedding: true };
        setDiarizationState(prev => ({ ...prev, status: 'completed', progress: 100 }));
      } else if (status.segmentation_available) {
        diarizationSubRef.current.segmentation = true;
      } else if (status.embedding_available) {
        diarizationSubRef.current.embedding = true;
      }
    }).catch(() => {});

    const unlistenProgress = listen<any>('diarization-download-progress', (event) => {
      const { progress, downloaded_mb, total_mb, speed_mbps } = event.payload;
      setDiarizationState(prev => ({
        ...prev,
        status: 'downloading',
        progress,
        downloadedMb: downloaded_mb ?? prev.downloadedMb,
        totalMb: total_mb ?? prev.totalMb,
        speedMbps: speed_mbps ?? prev.speedMbps,
      }));
    });

    const unlistenComplete = listen<any>('diarization-download-complete', (event) => {
      const { modelType } = event.payload;
      diarizationSubRef.current[modelType as 'segmentation' | 'embedding'] = true;
      const { segmentation, embedding } = diarizationSubRef.current;
      if (segmentation && embedding) {
        setDiarizationState(prev => ({ ...prev, status: 'completed', progress: 100 }));
      } else {
        setDiarizationState(prev => ({ ...prev, progress: 50 }));
      }
    });

    const unlistenError = listen<any>('diarization-download-error', (event) => {
      setDiarizationState(prev => ({
        ...prev,
        status: 'error',
        error: event.payload?.error ?? 'Download failed',
      }));
    });

    return () => {
      unlistenProgress.then(fn => fn());
      unlistenComplete.then(fn => fn());
      unlistenError.then(fn => fn());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Download trigger ───────────────────────────────────────────────────────

  const startDownloads = async () => {
    if (!parakeetDownloaded || !summaryModelDownloaded) {
      try {
        if (!parakeetDownloaded) setParakeetState(prev => ({ ...prev, status: 'downloading' }));
        if (!summaryModelDownloaded) setGemmaState(prev => ({ ...prev, status: 'downloading' }));
        await startBackgroundDownloads(true);
      } catch (error) {
        console.error('Failed to start downloads:', error);
        if (!parakeetDownloaded) setParakeetState(prev => ({ ...prev, status: 'error', error: String(error) }));
      }
    }
    if (!diarizationSubRef.current.segmentation || !diarizationSubRef.current.embedding) {
      setDiarizationState(prev => prev.status === 'completed' ? prev : { ...prev, status: 'downloading' });
      try {
        if (!diarizationSubRef.current.segmentation) {
          await invoke('diarization_download_model', { modelType: 'segmentation' });
        }
        if (!diarizationSubRef.current.embedding) {
          await invoke('diarization_download_model', { modelType: 'embedding' });
        }
      } catch (error) {
        console.error('Failed to start diarization downloads:', error);
        setDiarizationState(prev => prev.status !== 'completed' ? { ...prev, status: 'error', error: String(error) } : prev);
      }
    }
  };

  // ── Continue handler ───────────────────────────────────────────────────────

  const handleContinue = async () => {
    try {
      await invoke('parakeet_init');
      const actuallyAvailable = await invoke<boolean>('parakeet_has_available_models');
      if (actuallyAvailable && !parakeetDownloaded) {
        setParakeetDownloaded(true);
        setParakeetState(prev => ({ ...prev, status: 'completed', progress: 100 }));
      } else if (!actuallyAvailable && parakeetState.status === 'error') {
        toast.error('Transcription engine required', { description: 'Please retry the download before continuing.' });
        return;
      }
    } catch (error) {
      console.warn('[DownloadProgressStep] Failed to verify model:', error);
    }

    const downloadsComplete = parakeetState.status === 'completed' && gemmaState.status === 'completed';
    if (!downloadsComplete) {
      toast.info('Downloads will continue in the background', {
        description: 'You can start using the app. Recording will be available once speech recognition is ready.',
        duration: 5000,
      });
    }

    if (isMac) {
      goNext();
    } else {
      setIsCompleting(true);
      try {
        await completeOnboarding();
        await new Promise(resolve => setTimeout(resolve, 100));
        window.location.reload();
      } catch (error) {
        console.error('Failed to complete onboarding:', error);
        toast.error('Failed to complete setup', { description: 'Please try again.' });
        setIsCompleting(false);
      }
    }
  };

  // ── Download card renderer ─────────────────────────────────────────────────

  function renderDownloadCard(
    title: string,
    icon: React.ReactNode,
    state: DownloadState,
    modelSize: string,
    onRetry?: () => void,
  ) {
    return (
      <div className="bg-white rounded-xl border border-gray-200 p-5">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-full bg-gray-100 flex items-center justify-center">
              {icon}
            </div>
            <div>
              <h3 className="font-medium text-gray-900">{title}</h3>
              <p className="text-sm text-gray-500">{modelSize}</p>
            </div>
          </div>
          <div>
            {state.status === 'waiting' && <span className="text-sm text-gray-500">Waiting...</span>}
            {state.status === 'downloading' && <Loader2 className="w-5 h-5 text-gray-700 animate-spin" />}
            {state.status === 'completed' && (
              <div className="w-6 h-6 rounded-full bg-green-100 flex items-center justify-center">
                <Check className="w-4 h-4 text-green-600" />
              </div>
            )}
            {state.status === 'error' && <span className="text-sm text-red-500">Failed</span>}
          </div>
        </div>

        {(state.status === 'downloading' || state.status === 'completed') && (
          <div className="space-y-2">
            <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
              <div
                className="h-full bg-gradient-to-r from-gray-700 to-gray-900 rounded-full transition-all duration-300"
                style={{ width: `${state.progress}%` }}
              />
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="text-gray-600">
                {state.downloadedMb.toFixed(1)} MB / {state.totalMb.toFixed(1)} MB
              </span>
              <div className="flex items-center gap-2">
                {state.speedMbps > 0 && <span className="text-gray-500">{state.speedMbps.toFixed(1)} MB/s</span>}
                <span className="font-semibold text-gray-900">{Math.round(state.progress)}%</span>
              </div>
            </div>
          </div>
        )}

        {state.status === 'error' && state.error && (
          <div className="mt-2 p-3 bg-red-50 border border-red-200 rounded-md">
            <p className="text-sm text-red-600 font-medium">Download Error</p>
            <p className="text-xs text-red-500 mt-1">{state.error}</p>
            {onRetry && (
              <button
                onClick={onRetry}
                className="mt-3 w-full h-9 px-4 bg-gray-900 hover:bg-gray-800 text-white text-sm font-medium rounded-md transition-colors flex items-center justify-center gap-2"
              >
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                    d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                </svg>
                Try Again
              </button>
            )}
          </div>
        )}
      </div>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <OnboardingContainer
      title="Getting things ready"
      description="You can start using Clearminutes after downloading the Transcription Engine."
      step={3}
      totalSteps={isMac ? 4 : 3}
    >
      <div className="flex flex-col items-center space-y-6">
        <div className="flex flex-col items-center gap-2">
          <ClearMinutesIcon size={52} />
          <span
            className="text-lg font-extrabold tracking-tight text-gray-900 dark:text-foreground"
            style={{ fontFamily: 'var(--font-syne, system-ui)', letterSpacing: '-0.03em' }}
          >
            clearminutes
          </span>
        </div>

        <div className="w-full max-w-lg space-y-4">
          {renderDownloadCard('Transcription Engine', <Mic className="w-5 h-5 text-gray-600" />, parakeetState, '~670 MB', handleRetryDownload)}
          {renderDownloadCard('Summary Engine', <Sparkles className="w-5 h-5 text-gray-600" />, gemmaState, recommendedModel === 'gemma3:4b' ? '~2.5 GB' : '~806 MB', handleRetrySummaryDownload)}
          {renderDownloadCard('Speaker ID', <Users className="w-5 h-5 text-gray-600" />, diarizationState, '~217 MB', handleRetryDiarizationDownload)}
        </div>

        <AnimatePresence>
          {parakeetDownloaded && !summaryModelDownloaded && (
            <motion.div
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.3, ease: 'easeOut' }}
              className="w-full max-w-lg bg-gray-100 rounded-lg p-4 text-sm text-gray-800"
            >
              <div className="flex items-start gap-3">
                <Download className="w-5 h-5 text-gray-600 flex-shrink-0 mt-0.5" />
                <div>
                  <p className="font-medium">You can continue while this finishes</p>
                  <p className="text-gray-700 mt-1">Download will continue in the background.</p>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        <div className="w-full max-w-xs">
          <Button
            onClick={handleContinue}
            disabled={!parakeetDownloaded || isCompleting}
            className="w-full h-11 bg-gray-900 hover:bg-gray-800 text-white disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {(isCompleting || !parakeetDownloaded) ? (
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            ) : (
              'Continue'
            )}
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
