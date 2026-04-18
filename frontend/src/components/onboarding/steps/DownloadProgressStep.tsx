import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Check, Loader2, Mic } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { toast } from 'sonner';

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
    parakeetDownloaded,
    setParakeetDownloaded,
    startBackgroundDownloads,
    completeOnboarding,
  } = useOnboarding();

  const [isMac, setIsMac] = useState(false);
  const [isCompleting, setIsCompleting] = useState(false);
  const [parakeetState, setParakeetState] = useState<DownloadState>({
    status: parakeetDownloaded ? 'completed' : 'waiting',
    progress: parakeetDownloaded ? 100 : 0,
    downloadedMb: 0,
    totalMb: 670,
    speedMbps: 0,
  });
  const downloadStartedRef = useRef(false);

  useEffect(() => {
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };

    checkPlatform();
  }, []);

  useEffect(() => {
    if (downloadStartedRef.current) {
      return;
    }

    downloadStartedRef.current = true;
    if (!parakeetDownloaded) {
      setParakeetState((prev) => ({ ...prev, status: 'downloading' }));
    }
    startBackgroundDownloads().catch((error) => {
      console.error('Failed to start downloads:', error);
      setParakeetState((prev) => ({ ...prev, status: 'error', error: String(error) }));
    });
  }, [parakeetDownloaded, startBackgroundDownloads]);

  useEffect(() => {
    const unlistenProgress = listen<{
      modelName: string;
      progress: number;
      downloaded_mb?: number;
      total_mb?: number;
      speed_mbps?: number;
      status?: string;
    }>('parakeet-model-download-progress', (event) => {
      const { modelName, progress, downloaded_mb, total_mb, speed_mbps, status } = event.payload;
      if (modelName !== PARAKEET_MODEL) {
        return;
      }

      setParakeetState((prev) => ({
        ...prev,
        status: status === 'completed' ? 'completed' : 'downloading',
        progress,
        downloadedMb: downloaded_mb ?? prev.downloadedMb,
        totalMb: total_mb ?? prev.totalMb,
        speedMbps: speed_mbps ?? prev.speedMbps,
      }));

      if (status === 'completed' || progress >= 100) {
        setParakeetDownloaded(true);
      }
    });

    const unlistenComplete = listen<{ modelName: string }>('parakeet-model-download-complete', (event) => {
      if (event.payload.modelName !== PARAKEET_MODEL) {
        return;
      }

      setParakeetState((prev) => ({ ...prev, status: 'completed', progress: 100 }));
      setParakeetDownloaded(true);
    });

    const unlistenError = listen<{ modelName: string; error: string }>('parakeet-model-download-error', (event) => {
      if (event.payload.modelName !== PARAKEET_MODEL) {
        return;
      }

      setParakeetState((prev) => ({
        ...prev,
        status: 'error',
        error: event.payload.error,
      }));
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, [setParakeetDownloaded]);

  const handleContinue = async () => {
    try {
      await invoke('parakeet_init');
      const actuallyAvailable = await invoke<boolean>('parakeet_has_available_models');

      if (!actuallyAvailable) {
        toast.error('Transcription engine required', {
          description: 'Please wait for the download to finish before continuing.',
        });
        return;
      }
    } catch (error) {
      console.warn('[DownloadProgressStep] Failed to verify model:', error);
    }

    if (isMac) {
      goNext();
      return;
    }

    setIsCompleting(true);
    try {
      await completeOnboarding();
      await new Promise((resolve) => setTimeout(resolve, 100));
      window.location.reload();
    } catch (error) {
      console.error('Failed to complete onboarding:', error);
      toast.error('Failed to complete setup', {
        description: 'Please try again.',
      });
      setIsCompleting(false);
    }
  };

  return (
    <OnboardingContainer
      title="Downloading transcription engine"
      description="Meetily needs the local transcription engine before recording can start."
      step={3}
      totalSteps={isMac ? 4 : 3}
    >
      <div className="flex flex-col items-center space-y-6">
        <div className="w-full max-w-lg rounded-xl border border-gray-200 bg-white p-5">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-full bg-gray-100">
                <Mic className="h-5 w-5 text-gray-600" />
              </div>
              <div>
                <h3 className="font-medium text-gray-900">Transcription Engine</h3>
                <p className="text-sm text-gray-500">~670 MB</p>
              </div>
            </div>
            <div>
              {parakeetState.status === 'downloading' && <Loader2 className="h-5 w-5 animate-spin text-gray-700" />}
              {parakeetState.status === 'completed' && (
                <div className="flex h-6 w-6 items-center justify-center rounded-full bg-green-100">
                  <Check className="h-4 w-4 text-green-600" />
                </div>
              )}
              {parakeetState.status === 'waiting' && <span className="text-sm text-gray-500">Waiting...</span>}
              {parakeetState.status === 'error' && <span className="text-sm text-red-500">Failed</span>}
            </div>
          </div>

          {(parakeetState.status === 'downloading' || parakeetState.status === 'completed') && (
            <div className="space-y-2">
              <div className="h-2 w-full overflow-hidden rounded-full bg-gray-200">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-gray-700 to-gray-900 transition-all duration-300"
                  style={{ width: `${parakeetState.progress}%` }}
                />
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-gray-600">
                  {parakeetState.downloadedMb.toFixed(1)} MB / {parakeetState.totalMb.toFixed(1)} MB
                </span>
                <div className="flex items-center gap-2">
                  {parakeetState.speedMbps > 0 && (
                    <span className="text-gray-500">{parakeetState.speedMbps.toFixed(1)} MB/s</span>
                  )}
                  <span className="font-semibold text-gray-900">{Math.round(parakeetState.progress)}%</span>
                </div>
              </div>
            </div>
          )}

          {parakeetState.status === 'error' && parakeetState.error && (
            <div className="mt-2 rounded-md border border-red-200 bg-red-50 p-3">
              <p className="text-sm font-medium text-red-600">Download Error</p>
              <p className="mt-1 text-xs text-red-500">{parakeetState.error}</p>
            </div>
          )}
        </div>

        <div className="w-full max-w-xs">
          <Button
            onClick={handleContinue}
            disabled={!parakeetDownloaded || isCompleting}
            className="h-11 w-full bg-gray-900 text-white hover:bg-gray-800 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isCompleting ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : 'Continue'}
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
