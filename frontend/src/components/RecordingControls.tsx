'use client';

import { useCallback, useState, useEffect, useRef } from 'react';
import { useSession } from 'next-auth/react';
import { Square, Mic, AlertCircle, X, Pause, Play } from 'lucide-react';
import { SummaryResponse } from '@/types/summary';
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import Analytics from '@/lib/analytics';
import { wsUrl } from '@/lib/config';
import { AudioStreamClient } from '@/lib/audio-streaming/AudioStreamClient';
import { GroqApiKeyDialog } from './GroqApiKeyDialog';
import { authFetch } from '@/lib/api';

interface RecordingControlsProps {
  isRecording: boolean;
  barHeights: string[];
  onRecordingStop: (callApi?: boolean) => void;
  onRecordingStart: () => void;
  onTranscriptReceived: (summary: SummaryResponse) => void;
  onTranscriptionError?: (message: string, code?: string) => void;
  onStopInitiated?: () => void;
  isRecordingDisabled: boolean;
  isParentProcessing: boolean;
  selectedDevices?: {
    micDevice: string | null;
    systemDevice: string | null;
  };
  meetingName?: string;
  onSessionIdReceived?: (sessionId: string) => void;
  initialSessionId?: string | null;
  onPauseChange?: (paused: boolean) => void;
  startSignal?: number;
}

export const RecordingControls: React.FC<RecordingControlsProps> = ({
  isRecording,
  barHeights,
  onRecordingStop,
  onRecordingStart,
  onTranscriptReceived,
  onTranscriptionError,
  onStopInitiated,
  isRecordingDisabled,
  isParentProcessing,
  onSessionIdReceived,
  initialSessionId,
  onPauseChange,
  startSignal
}) => {
  const [isProcessing, setIsProcessing] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [deviceError, setDeviceError] = useState<{ title: string, message: string } | null>(null);
  const [sessionError, setSessionError] = useState<boolean>(false);
  const [showApiKeyDialog, setShowApiKeyDialog] = useState(false);
  const { data: session } = useSession();

  // Real-time streaming audio client
  const audioClientRef = useRef<AudioStreamClient | null>(null);
  const lastStartSignalRef = useRef<number | undefined>(undefined);

  // Debug: Log when component mounts
  useEffect(() => {
    console.log('✅ [RecordingControlsWeb] Component mounted');
    console.log('📋 [RecordingControlsWeb] Props:', {
      isRecording,
      isRecordingDisabled,
      isParentProcessing
    });

    return () => {
      if (audioClientRef.current) {
        console.log('🧹 [RecordingControls] Unmounting - cleaning up audio client');
        void audioClientRef.current.stop();
        audioClientRef.current = null;
      }
    };
  }, []);

  // Extra safety: force-stop on tab close/reload to avoid orphan live streams.
  useEffect(() => {
    const forceStopOnUnload = () => {
      if (audioClientRef.current) {
        void audioClientRef.current.stop();
        audioClientRef.current = null;
      }
    };

    window.addEventListener('beforeunload', forceStopOnUnload);
    window.addEventListener('pagehide', forceStopOnUnload);
    return () => {
      window.removeEventListener('beforeunload', forceStopOnUnload);
      window.removeEventListener('pagehide', forceStopOnUnload);
    };
  }, []);

  // Listen for session expiry events from api.ts
  useEffect(() => {
    const handleSessionExpired = () => {
      console.warn('⚠️ Session expired during recording/usage');
      setSessionError(true);
    };

    window.addEventListener('auth:session-expired', handleSessionExpired);
    return () => window.removeEventListener('auth:session-expired', handleSessionExpired);
  }, []);

  const handleStartRecording = useCallback(async () => {
    if (isStarting) {
      console.log('⚠️ Already starting, ignoring duplicate click');
      return;
    }

    console.log('🎙️ [RecordingControls] Starting real-time streaming transcription...');
    setIsStarting(true);
    setIsPaused(false);
    setDeviceError(null);

    try {
      // Create new streaming audio client (uses Groq Whisper)
      const client = new AudioStreamClient(wsUrl);
      audioClientRef.current = client;
      const stableSessionId =
        initialSessionId ||
        (typeof crypto !== 'undefined' && 'randomUUID' in crypto
          ? crypto.randomUUID()
          : `session-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`);

      // Persist session immediately so recovery/save paths have one canonical id.
      if (!initialSessionId && onSessionIdReceived) {
        onSessionIdReceived(stableSessionId);
      }

      await client.start({
        onConnected: (sessionId) => {
          console.log('✅ Connected to streaming service, session:', sessionId);
          if (onSessionIdReceived) onSessionIdReceived(sessionId);
        },

        onPartial: (text, confidence, isStable) => {
          // Partial transcripts disabled by backend
        },

        onFinal: (text, confidence, reason, timing) => {
          // Final transcripts (black, locked) - main content
          console.log('📝 [RecordingControls] Final transcript:', text.substring(0, 50) + '...');

          const transcriptUpdate = {
            text: text,
            timestamp: new Date().toISOString(),
            sequence_id: Date.now(),
            is_partial: false,
            audio_start_time: timing?.start,
            audio_end_time: timing?.end,
            duration: timing?.duration
          };

          if (onTranscriptReceived) {
            onTranscriptReceived(transcriptUpdate as any);
            console.log('✅ Transcript passed to parent component');
          }
        },

        onError: (error, code) => {
          console.error('❌ Streaming error:', error, 'Code:', code);

          // Handle GROQ_KEY_REQUIRED specifically
          if (code === 'GROQ_KEY_REQUIRED') {
            setShowApiKeyDialog(true);
            // Don't show the error alert, as the dialog will handle it
            return; 
          }

          onTranscriptionError?.(error.message, code);
        },

        onDisconnected: () => {
          console.log('🔌 Streaming disconnected');
        }
      }, stableSessionId, stableSessionId, (session as any)?.idToken || undefined);

      console.log('✅ Real-time streaming started');

      // Notify parent component
      onRecordingStart();

      Analytics.trackButtonClick('start_recording_streaming', 'recording_controls');
    } catch (error) {
      console.error('❌ Failed to start streaming transcription:', error);

      const errorMsg = error instanceof Error ? error.message : String(error);

      if (errorMsg.includes('denied') || errorMsg.includes('permission')) {
        setDeviceError({
          title: 'Microphone Permission Required',
          message: 'Please grant microphone access in your browser and try again.'
        });
      } else if (errorMsg.includes('NotFoundError') || errorMsg.includes('no microphone')) {
        setDeviceError({
          title: 'Microphone Not Found',
          message: 'No microphone detected. Please connect a microphone and try again.'
        });
      } else if (errorMsg.includes('WebSocket') || errorMsg.includes('connect')) {
        setDeviceError({
          title: 'Connection Failed',
          message: 'Unable to connect to transcription service. Please check that the backend is running.'
        });
      } else {
        setDeviceError({
          title: 'Recording Failed',
          message: `Failed to start recording: ${errorMsg}`
        });
      }

      onTranscriptionError?.(errorMsg);
    } finally {
      setIsStarting(false);
    }
  }, [onRecordingStart, onTranscriptionError, onTranscriptReceived, isStarting, initialSessionId, onSessionIdReceived, session?.user?.email]);

  // External start trigger (used by recovery resume action).
  useEffect(() => {
    if (startSignal === undefined) return;
    if (lastStartSignalRef.current === startSignal) return;
    lastStartSignalRef.current = startSignal;
    if (startSignal <= 0) return;
    if (isStarting || isStopping) return;
    if (audioClientRef.current?.isActive()) return;
    handleStartRecording();
  }, [startSignal, isStarting, isStopping, handleStartRecording]);

  const handleStopRecording = useCallback(async () => {
    if (!isRecording || isStarting || isStopping) {
      console.log('⚠️ Cannot stop recording (invalid state)');
      return;
    }

    console.log('🛑 Stopping streaming transcription...');

    onStopInitiated?.();
    setIsStopping(true);

    try {
      // Stop the streaming audio client
      await audioClientRef.current?.stop();
      audioClientRef.current = null;
      console.log('✅ Streaming stopped');

      setIsProcessing(false);
      onRecordingStop(true);

      Analytics.trackButtonClick('stop_recording_streaming', 'recording_controls');
    } catch (error) {
      console.error('❌ Failed to stop streaming:', error);
      onRecordingStop(false);
    } finally {
      setIsStopping(false);
    }

  }, [isRecording, isStarting, isStopping, onStopInitiated, onRecordingStop]);

  const handlePauseResume = useCallback(async () => {
    if (!audioClientRef.current) return;

    try {
      if (isPaused) {
        await audioClientRef.current.resume();
        setIsPaused(false);
        onPauseChange?.(false);
        console.log('▶️ Resumed recording');
      } else {
        await audioClientRef.current.pause();
        setIsPaused(true);
        onPauseChange?.(true);
        console.log('⏸️ Paused recording');
      }
    } catch (error) {
      console.error('Failed to toggle pause/resume:', error);
    }
  }, [isPaused, onPauseChange]);

  const handleSaveApiKey = async (apiKey: string) => {
    try {
      const response = await authFetch('/api/user/keys', {
        method: 'POST',
        body: JSON.stringify({
          provider: 'groq',
          api_key: apiKey,
        }),
      });

      if (!response.ok) {
        const data = await response.json();
        throw new Error(data.detail || 'Failed to save API key');
      }

      // Success
      console.log('✅ Groq API key saved successfully');
      
      // Retry starting recording
      setTimeout(() => {
        handleStartRecording();
      }, 500); // Small delay to ensure DB update propagates if needed
      
    } catch (error) {
      console.error('Failed to save API key:', error);
      throw error; // Re-throw to be caught by the dialog
    }
  };

  return (
    <TooltipProvider>
      <GroqApiKeyDialog 
        isOpen={showApiKeyDialog} 
        onClose={() => setShowApiKeyDialog(false)} 
        onSave={handleSaveApiKey}
      />
      <div className="flex flex-col space-y-2">
        <div className="flex items-center space-x-2 bg-white rounded-full shadow-lg px-4 py-2">
          {isProcessing && !isParentProcessing ? (
            <div className="flex items-center space-x-2">
              <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-gray-900"></div>
              <span className="text-sm text-gray-600">Processing recording...</span>
            </div>
          ) : (
            <>
              {!isRecording ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      onClick={() => {
                        Analytics.trackButtonClick('start_recording', 'recording_controls');
                        handleStartRecording();
                      }}
                      disabled={isStarting || isProcessing || isRecordingDisabled}
                      className={`w-12 h-12 flex items-center justify-center ${isStarting || isProcessing ? 'bg-gray-400' : 'bg-red-500 hover:bg-red-600'
                        } rounded-full text-white transition-colors relative`}
                    >
                      {isStarting ? (
                        <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-white"></div>
                      ) : (
                        <Mic size={20} />
                      )}
                    </button>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>Start recording</p>
                  </TooltipContent>
                </Tooltip>
              ) : (
                <div className="flex items-center space-x-2">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          handlePauseResume();
                        }}
                        className={`w-10 h-10 flex items-center justify-center ${isPaused ? 'bg-amber-500 hover:bg-amber-600' : 'bg-blue-500 hover:bg-blue-600'
                          } rounded-full text-white transition-colors`}
                      >
                        {isPaused ? <Play size={16} /> : <Pause size={16} />}
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>{isPaused ? 'Resume recording' : 'Pause recording'}</p>
                    </TooltipContent>
                  </Tooltip>

                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        onClick={() => {
                          Analytics.trackButtonClick('stop_recording', 'recording_controls');
                          handleStopRecording();
                        }}
                        disabled={isStopping}
                        className={`w-10 h-10 flex items-center justify-center ${isStopping ? 'bg-gray-400' : 'bg-red-500 hover:bg-red-600'
                          } rounded-full text-white transition-colors relative`}
                      >
                        <Square size={16} />
                        {isStopping && (
                          <div className="absolute -top-8 text-gray-600 font-medium text-xs">
                            Stopping...
                          </div>
                        )}
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Stop recording</p>
                    </TooltipContent>
                  </Tooltip>
                </div>
              )}

              <div className="flex items-center space-x-1 mx-4">
                {barHeights.map((height, index) => (
                  <div
                    key={index}
                    className="w-1 rounded-full transition-all duration-200 bg-red-500"
                    style={{
                      height: isRecording ? height : '4px',
                    }}
                  />
                ))}
              </div>
            </>
          )}
        </div >

        {sessionError && (
          <Alert className="mt-4 border-yellow-300 bg-yellow-50">
            <AlertCircle className="h-5 w-5 text-yellow-600" />
            <button
              onClick={() => setSessionError(false)}
              className="absolute right-3 top-3 text-yellow-600 hover:text-yellow-800 transition-colors"
              aria-label="Close alert"
            >
              <X className="h-4 w-4" />
            </button>
            <AlertTitle className="text-yellow-800 font-semibold mb-1">
              Session Expired
            </AlertTitle>
            <AlertDescription className="text-yellow-700 text-sm">
              Your login session has expired. Recording will continue, but you may need to log in again in a new tab to save or view transcripts.
            </AlertDescription>
          </Alert>
        )}

        {
          deviceError && (
            <Alert variant="destructive" className="mt-4 border-red-300 bg-red-50">
              <AlertCircle className="h-5 w-5 text-red-600" />
              <button
                onClick={() => setDeviceError(null)}
                className="absolute right-3 top-3 text-red-600 hover:text-red-800 transition-colors"
                aria-label="Close alert"
              >
                <X className="h-4 w-4" />
              </button>
              <AlertTitle className="text-red-800 font-semibold mb-2">
                {deviceError.title}
              </AlertTitle>
              <AlertDescription className="text-red-700">
                {deviceError.message.split('\n').map((line, i) => (
                  <div key={i} className={i > 0 ? 'ml-2' : ''}>
                    {line}
                  </div>
                ))}
              </AlertDescription>
            </Alert>
          )
        }
      </div >
    </TooltipProvider >
  );
};
