import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Mic, Sparkles, Check, Loader2, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { DEFAULT_COHERE_MODEL } from '@/lib/cohere';
import { toast } from 'sonner';
import { motion, AnimatePresence } from 'framer-motion';

const COHERE_MODEL = DEFAULT_COHERE_MODEL;

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
    cohereDownloaded,
    setCohereDownloaded,
    summaryModelDownloaded,
    setSummaryModelDownloaded,
    startBackgroundDownloads,
    completeOnboarding,
  } = useOnboarding();

  const [recommendedModel, setRecommendedModel] = useState<string>('gemma3:1b');
  const [isMac, setIsMac] = useState(false);

  const [cohereState, setCohereState] = useState<DownloadState>({
    status: cohereDownloaded ? 'completed' : 'waiting',
    progress: cohereDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 2048,
    speedMbps: 0,
  });

  const [gemmaState, setGemmaState] = useState<DownloadState>({
    status: summaryModelDownloaded ? 'completed' : 'waiting',
    progress: summaryModelDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 806, // 1b model size
    speedMbps: 0,
  });

  const [isCompleting, setIsCompleting] = useState(false);
  const downloadStartedRef = useRef(false);
  const retryingRef = useRef(false);
  const retryingSummaryRef = useRef(false);

  // Retry download handler
  const handleRetryDownload = async () => {
    // Prevent multiple simultaneous retries
    if (retryingRef.current) {
      console.log('[DownloadProgressStep] Retry already in progress, ignoring');
      return;
    }

    console.log('[DownloadProgressStep] Retrying Cohere download');
    retryingRef.current = true;

    // Reset error state
    setCohereState((prev) => ({
      ...prev,
      status: 'waiting',
      error: undefined,
      progress: 0,
      downloadedMb: 0,
      speedMbps: 0,
    }));

    try {
      // Cancel any in-flight download, then re-invoke the download command.
      try {
        await invoke('cohere_cancel_download');
      } catch (cancelErr) {
        console.log('[DownloadProgressStep] No active Cohere download to cancel:', cancelErr);
      }
      await invoke('cohere_download_model', { modelName: COHERE_MODEL });
      // Progress events will update state
    } catch (error) {
      console.error('[DownloadProgressStep] Retry failed:', error);
      setCohereState((prev) => ({
        ...prev,
        status: 'error',
        error: error instanceof Error ? error.message : '재시도에 실패했습니다',
      }));

      toast.error('다운로드 재시도에 실패했습니다', {
        description: '연결 상태를 확인한 후 다시 시도해 주세요.',
      });
    } finally {
      // Allow retry again after 2 seconds
      setTimeout(() => {
        retryingRef.current = false;
      }, 2000);
    }
  };

  // Retry summary download handler
  const handleRetrySummaryDownload = async () => {
    // Prevent multiple simultaneous retries
    if (retryingSummaryRef.current) {
      console.log('[DownloadProgressStep] Summary retry already in progress, ignoring');
      return;
    }

    console.log('[DownloadProgressStep] Retrying summary model download');
    retryingSummaryRef.current = true;

    // Reset error state
    setGemmaState((prev) => ({
      ...prev,
      status: 'downloading',
      error: undefined,
      progress: 0,
      downloadedMb: 0,
      speedMbps: 0,
    }));

    try {
      // Call download command directly (no retry command exists for built-in AI)
      await invoke('builtin_ai_download_model', { modelName: selectedSummaryModel || recommendedModel });
    } catch (error) {
      console.error('[DownloadProgressStep] Summary retry failed:', error);
      setGemmaState((prev) => ({
        ...prev,
        status: 'error',
        error: error instanceof Error ? error.message : '재시도에 실패했습니다',
      }));

      toast.error('요약 모델 다운로드 재시도에 실패했습니다', {
        description: '연결 상태를 확인한 후 다시 시도해 주세요.',
      });
    } finally {
      // Allow retry again after 2 seconds
      setTimeout(() => {
        retryingSummaryRef.current = false;
      }, 2000);
    }
  };

  // Fetch recommended model and detect platform on mount
  useEffect(() => {
    const fetchRecommendation = async () => {
      try {
        const model = await invoke<string>('builtin_ai_get_recommended_model');
        setRecommendedModel(model);
        setSelectedSummaryModel(model);  // Update context
      } catch (error) {
        console.error('Failed to get recommended model:', error);
        // Keep default gemma3:1b
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

  // Start downloads on mount
  useEffect(() => {
    if (downloadStartedRef.current) return;
    downloadStartedRef.current = true;

    startDownloads();
  }, []);

  // Listen to Cohere download progress
  useEffect(() => {
    const unlistenProgress = listen<{
      modelName: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status?: string;
    }>('cohere-download-progress', (event) => {
      const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
      if (modelName === COHERE_MODEL) {
        setCohereState((prev) => ({
          ...prev,
          status: status === 'completed' ? 'completed' : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
        }));

        if (status === 'completed' || progress >= 100) {
          setCohereDownloaded(true);
        }
      }
    });

    const unlistenComplete = listen<{ modelName: string }>(
      'cohere-download-complete',
      (event) => {
        if (event.payload.modelName === COHERE_MODEL) {
          setCohereState((prev) => ({ ...prev, status: 'completed', progress: 100 }));
          setCohereDownloaded(true);
        }
      }
    );

    const unlistenError = listen<{ modelName: string; error: string }>(
      'cohere-download-error',
      (event) => {
        if (event.payload.modelName === COHERE_MODEL) {
          setCohereState((prev) => ({
            ...prev,
            status: 'error',
            error: event.payload.error,
          }));
        }
      }
    );

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  // Listen to Gemma download progress (always downloading for builtin-ai)
  useEffect(() => {
    const unlisten = listen<{
      model: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status: string;
      error?: string;
    }>('builtin-ai-download-progress', (event) => {
      const { model, progress, downloaded_mb, total_mb, speed_mbps, status, error } = event.payload;
      if (model === selectedSummaryModel || model === 'gemma3:1b' || model === 'gemma3:4b') {
        setGemmaState((prev) => ({
          ...prev,
          status: status === 'completed'
            ? 'completed'
            : status === 'error'
            ? 'error'
            : 'downloading',
          progress,
          downloadedMb: downloaded_mb ?? prev.downloadedMb,
          totalMb: total_mb ?? prev.totalMb,
          speedMbps: speed_mbps ?? prev.speedMbps,
          error: status === 'error' ? error : undefined,
        }));

        if (status === 'completed' || progress >= 100) {
          setSummaryModelDownloaded(true);
        }
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [selectedSummaryModel]);

  const startDownloads = async () => {
    // Always download both Cohere and Gemma (system-recommended)
    if (!cohereDownloaded || !summaryModelDownloaded) {
      try {
        if (!cohereDownloaded) {
          setCohereState((prev) => ({ ...prev, status: 'downloading' }));
        }
        if (!summaryModelDownloaded) {
          setGemmaState((prev) => ({ ...prev, status: 'downloading' }));
        }
        await startBackgroundDownloads(true);  // Always download both
      } catch (error) {
        console.error('Failed to start downloads:', error);
        if (!cohereDownloaded) {
          setCohereState((prev) => ({ ...prev, status: 'error', error: String(error) }));
        }
      }
    }
  };

  const handleContinue = async () => {
    // Verify actual model availability (catches state drift)
    try {
      await invoke('cohere_init');
      const actuallyAvailable = await invoke<boolean>('cohere_is_model_loaded');

      if (actuallyAvailable && !cohereDownloaded) {
        console.log('[DownloadProgressStep] Model available but state not updated');
        setCohereDownloaded(true);
        setCohereState((prev) => ({
          ...prev,
          status: 'completed',
          progress: 100,
        }));
      } else if (!actuallyAvailable && cohereState.status === 'error') {
        toast.error('전사 엔진이 필요합니다', {
          description: '계속 진행하기 전에 다운로드를 다시 시도해 주세요.',
        });
        return;
      }
    } catch (error) {
      console.warn('[DownloadProgressStep] Failed to verify model:', error);
    }

    // Check if downloads are complete for toast notification
    const downloadsComplete = cohereState.status === 'completed' &&
      gemmaState.status === 'completed';

    // Show toast if downloads still in progress
    if (!downloadsComplete) {
      toast.info('백그라운드에서 다운로드를 계속 진행합니다', {
        description: '앱을 바로 사용할 수 있습니다. 음성 인식 준비가 완료되면 녹음이 가능해집니다.',
        duration: 5000,
      });
    }

    if (isMac) {
      // macOS: Go to Permissions step (will complete after permissions granted)
      goNext();
    } else {
      // Non-macOS: Complete onboarding immediately (downloads continue in background)
      setIsCompleting(true);
      try {
        await completeOnboarding();

        // Small delay to ensure state is saved before reload
        await new Promise(resolve => setTimeout(resolve, 100));

        window.location.reload();
      } catch (error) {
        console.error('Failed to complete onboarding:', error);
        toast.error('설정을 완료하지 못했습니다', {
          description: '다시 시도해 주세요.',
        });
        setIsCompleting(false);
      }
    }
  };

  const renderDownloadCard = (
    title: string,
    icon: React.ReactNode,
    state: DownloadState,
    modelSize: string
  ) => (
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
          {state.status === 'waiting' && (
            <span className="text-sm text-gray-500">대기 중...</span>
          )}
          {state.status === 'downloading' && (
            <Loader2 className="w-5 h-5 text-gray-700 animate-spin" />
          )}
          {state.status === 'completed' && (
            <div className="w-6 h-6 rounded-full bg-green-100 flex items-center justify-center">
              <Check className="w-4 h-4 text-green-600" />
            </div>
          )}
          {state.status === 'error' && (
            <span className="text-sm text-red-500">실패</span>
          )}
        </div>
      </div>

      {/* Progress Bar */}
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
              {state.speedMbps > 0 && (
                <span className="text-gray-500">
                  {state.speedMbps.toFixed(1)} MB/s
                </span>
              )}
              <span className="font-semibold text-gray-900">
                {Math.round(state.progress)}%
              </span>
            </div>
          </div>
        </div>
      )}

      {state.status === 'error' && state.error && (
        <div className="mt-2 p-3 bg-red-50 border border-red-200 rounded-md">
          <p className="text-sm text-red-600 font-medium">다운로드 오류</p>
          <p className="text-xs text-red-500 mt-1">{state.error}</p>
          {(title === '전사 엔진' || title === '요약 엔진') && (
            <button
              onClick={title === '전사 엔진' ? handleRetryDownload : handleRetrySummaryDownload}
              className="mt-3 w-full h-9 px-4 bg-gray-900 hover:bg-gray-800 text-white text-sm font-medium rounded-md transition-colors flex items-center justify-center gap-2"
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2}
                      d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              다시 시도
            </button>
          )}
        </div>
      )}
    </div>
  );

  return (
    <OnboardingContainer
      title="준비 중"
      description="전사 엔진 다운로드가 끝나면 Meetily를 사용할 수 있습니다."
      step={3}
      totalSteps={isMac ? 4 : 3}
    >
      <div className="flex flex-col items-center space-y-6">
        {/* Download Cards */}
        <div className="w-full max-w-lg space-y-4">
          {renderDownloadCard(
            '전사 엔진',
            <Mic className="w-5 h-5 text-gray-600" />,
            cohereState,
            '~1.5–2.5 GB'
          )}

          {renderDownloadCard(
            '요약 엔진',
            <Sparkles className="w-5 h-5 text-gray-600" />,
            gemmaState,
            recommendedModel === 'gemma3:4b' ? '~2.5 GB' : '~806 MB'
          )}
        </div>

        {/* Info Message - Only show when Cohere is downloaded */}
        <AnimatePresence>
          {cohereDownloaded && !summaryModelDownloaded && (
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
                  <p className="font-medium">다운로드가 끝날 때까지 기다리지 않고 계속 진행할 수 있습니다</p>
                  <p className="text-gray-700 mt-1">
                    백그라운드에서 다운로드가 계속됩니다.
                  </p>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Continue Button */}
        <div className="w-full max-w-xs">
          <Button
            onClick={handleContinue}
            disabled={!cohereDownloaded || isCompleting}
            className="w-full h-11 bg-gray-900 hover:bg-gray-800 text-white disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {(isCompleting || !cohereDownloaded) ? (
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
            ) : (
              '계속'
            )}
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
