'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import Sidebar from '@/components/Sidebar';
import { SidebarProvider } from '@/components/Sidebar/SidebarProvider';
import MainContent from '@/components/MainContent';
import AnalyticsProvider from '@/components/AnalyticsProvider';
import { Toaster, toast } from 'sonner';
import 'sonner/dist/styles.css';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { TooltipProvider } from '@/components/ui/tooltip';
import { RecordingStateProvider } from '@/contexts/RecordingStateContext';
import { OllamaDownloadProvider } from '@/contexts/OllamaDownloadContext';
import { TranscriptProvider } from '@/contexts/TranscriptContext';
import { SpeakerProvider } from '@/contexts/SpeakerContext';
import { ConfigProvider, useConfig } from '@/contexts/ConfigContext';
import { OnboardingProvider } from '@/contexts/OnboardingContext';
import { OnboardingFlow } from '@/components/onboarding';
import { loadBetaFeatures } from '@/types/betaFeatures';
import { DownloadProgressToastProvider } from '@/components/shared/DownloadProgressToast';
import { UpdateCheckProvider } from '@/components/UpdateCheckProvider';
import { RecordingPostProcessingProvider } from '@/contexts/RecordingPostProcessingProvider';
import { ImportAudioDialog, ImportDropOverlay } from '@/components/ImportAudio';
import { ImportDialogProvider } from '@/contexts/ImportDialogContext';
import { isAudioExtension, getAudioFormatsDisplayList } from '@/constants/audioFormats';
import { ThemeProvider, useTheme } from '@/contexts/ThemeContext';

function ConditionalImportDialog({
  showImportDialog,
  handleImportDialogClose,
  importFilePath,
}: {
  showImportDialog: boolean;
  handleImportDialogClose: (open: boolean) => void;
  importFilePath: string | null;
}) {
  const { betaFeatures } = useConfig();
  if (!betaFeatures.importAndRetranscribe) return null;
  return (
    <ImportAudioDialog
      open={showImportDialog}
      onOpenChange={handleImportDialogClose}
      preselectedFile={importFilePath}
    />
  );
}

function ThemedToaster() {
  const { isDark } = useTheme();
  return (
    <Toaster
      position="top-right"
      closeButton
      expand={false}
      theme={isDark ? 'dark' : 'light'}
      toastOptions={{
        classNames: {
          toast: 'flex items-start gap-3 !bg-white dark:!bg-[#1e2028] !border !border-gray-200 dark:!border-white/10 !shadow-lg !rounded-lg !p-3 !text-gray-900 dark:!text-gray-100',
          title: '!text-sm !font-medium !text-gray-900 dark:!text-gray-100',
          description: '!text-xs !text-gray-500 dark:!text-gray-400',
          error: '!bg-white dark:!bg-[#1e2028] !border-red-200 dark:!border-red-900',
          success: '!bg-white dark:!bg-[#1e2028] !border-green-200 dark:!border-green-900',
          warning: '!bg-white dark:!bg-[#1e2028] !border-yellow-200 dark:!border-yellow-900',
          info: '!bg-white dark:!bg-[#1e2028] !border-blue-200 dark:!border-blue-900',
          icon: '!mt-0',
          closeButton: '!top-2 !right-2',
        },
      }}
    />
  );
}

export default function AppShell({ children }: { children: React.ReactNode }) {
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [onboardingCompleted, setOnboardingCompleted] = useState(false);
  const [showDropOverlay, setShowDropOverlay] = useState(false);
  const isInternalDragging = useRef(false);
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [importFilePath, setImportFilePath] = useState<string | null>(null);

  useEffect(() => {
    invoke<{ completed: boolean } | null>('get_onboarding_status')
      .then((status) => {
        const isComplete = status?.completed ?? false;
        setOnboardingCompleted(isComplete);
        setShowOnboarding(!isComplete);
      })
      .catch(() => {
        setShowOnboarding(true);
        setOnboardingCompleted(false);
      });
  }, []);

  useEffect(() => {
    if (process.env.NODE_ENV === 'production') {
      const handleContextMenu = (e: MouseEvent) => e.preventDefault();
      document.addEventListener('contextmenu', handleContextMenu);
      return () => document.removeEventListener('contextmenu', handleContextMenu);
    }
  }, []);

  useEffect(() => {
    const unlisten = listen('request-recording-toggle', () => {
      if (showOnboarding) {
        toast.error('Please complete setup first', {
          description: 'You need to finish onboarding before you can start recording.',
        });
      } else {
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [showOnboarding]);

  const handleFileDrop = useCallback((paths: string[]) => {
    const betaFeatures = loadBetaFeatures();
    if (!betaFeatures.importAndRetranscribe) {
      toast.error('Beta feature disabled', {
        description: 'Enable "Import Audio & Retranscribe" in Settings > Beta to use this feature.',
      });
      return;
    }
    const audioFile = paths.find(p => {
      const ext = p.split('.').pop()?.toLowerCase();
      return !!ext && isAudioExtension(ext);
    });
    if (audioFile) {
      setImportFilePath(audioFile);
      setShowImportDialog(true);
    } else if (paths.length > 0) {
      toast.error('Please drop an audio file', {
        description: `Supported formats: ${getAudioFormatsDisplayList()}`,
      });
    }
  }, []);

  // Track internal HTML drag-and-drop (e.g. section reordering) to suppress the file import overlay
  useEffect(() => {
    const onDragStart = () => {
      isInternalDragging.current = true;
      // Only update state if overlay was showing (avoids unnecessary re-renders during BlockNote drags)
      setShowDropOverlay(prev => prev ? false : prev);
    };
    const onDragEnd = () => { isInternalDragging.current = false; };
    document.addEventListener('dragstart', onDragStart);
    document.addEventListener('dragend', onDragEnd);
    return () => {
      document.removeEventListener('dragstart', onDragStart);
      document.removeEventListener('dragend', onDragEnd);
    };
  }, []);

  useEffect(() => {
    if (showOnboarding) return;
    const unlisteners: UnlistenFn[] = [];
    const cleanedUpRef = { current: false };

    const setupListeners = async () => {
      const unlistenDragEnter = await listen('tauri://drag-enter', () => {
        if (!isInternalDragging.current && loadBetaFeatures().importAndRetranscribe) setShowDropOverlay(true);
      });
      if (cleanedUpRef.current) { unlistenDragEnter(); return; }
      unlisteners.push(unlistenDragEnter);

      const unlistenDragLeave = await listen('tauri://drag-leave', () => setShowDropOverlay(false));
      if (cleanedUpRef.current) { unlistenDragLeave(); unlisteners.forEach(u => u()); return; }
      unlisteners.push(unlistenDragLeave);

      const unlistenDrop = await listen<{ paths: string[] }>('tauri://drag-drop', (event) => {
        setShowDropOverlay(false);
        handleFileDrop(event.payload.paths);
      });
      if (cleanedUpRef.current) { unlistenDrop(); unlisteners.forEach(u => u()); return; }
      unlisteners.push(unlistenDrop);
    };

    setupListeners();
    return () => {
      cleanedUpRef.current = true;
      unlisteners.forEach(u => u());
    };
  }, [showOnboarding, handleFileDrop]);

  const handleImportDialogClose = useCallback((open: boolean) => {
    setShowImportDialog(open);
    if (!open) setImportFilePath(null);
  }, []);

  const handleOpenImportDialog = useCallback((filePath?: string | null) => {
    setImportFilePath(filePath ?? null);
    setShowImportDialog(true);
  }, []);

  const handleOnboardingComplete = () => {
    setShowOnboarding(false);
    setOnboardingCompleted(true);
    window.location.reload();
  };

  return (
    <ThemeProvider>
      <AnalyticsProvider>
        <RecordingStateProvider>
          <TranscriptProvider>
            <SpeakerProvider>
              <ConfigProvider>
                <OllamaDownloadProvider>
                  <OnboardingProvider>
                    <UpdateCheckProvider>
                      <SidebarProvider>
                        <TooltipProvider>
                          <RecordingPostProcessingProvider>
                            <ImportDialogProvider onOpen={handleOpenImportDialog}>
                              <DownloadProgressToastProvider />
                              {showOnboarding ? (
                                <OnboardingFlow onComplete={handleOnboardingComplete} />
              ) : (
                <div className="flex">
                  <Sidebar />
                  <MainContent>{children}</MainContent>
                </div>
                              )}
                              <ImportDropOverlay visible={showDropOverlay} />
                              <ConditionalImportDialog
                                showImportDialog={showImportDialog}
                                handleImportDialogClose={handleImportDialogClose}
                                importFilePath={importFilePath}
                              />
                            </ImportDialogProvider>
                          </RecordingPostProcessingProvider>
                        </TooltipProvider>
                      </SidebarProvider>
                    </UpdateCheckProvider>
                  </OnboardingProvider>
                </OllamaDownloadProvider>
              </ConfigProvider>
            </SpeakerProvider>
          </TranscriptProvider>
        </RecordingStateProvider>
      </AnalyticsProvider>
      <ThemedToaster />
    </ThemeProvider>
  );
}

