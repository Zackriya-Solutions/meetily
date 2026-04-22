import React from 'react';
import { AlertTriangle, Mic, Speaker, RefreshCw } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { invoke } from '@tauri-apps/api/core';
import { useIsLinux } from '@/hooks/usePlatform';

interface PermissionWarningProps {
  hasMicrophone: boolean;
  hasSystemAudio: boolean;
  onRecheck: () => void;
  isRechecking?: boolean;
}

export function PermissionWarning({
  hasMicrophone,
  hasSystemAudio,
  onRecheck,
  isRechecking = false
}: PermissionWarningProps) {
  const isLinux = useIsLinux();

  // Don't show on Linux - permission handling is not needed
  if (isLinux) {
    return null;
  }

  // Don't show if both permissions are granted
  if (hasMicrophone && hasSystemAudio) {
    return null;
  }

  const isMacOS = navigator.userAgent.includes('Mac');

  const openMicrophoneSettings = async () => {
    if (isMacOS) {
      try {
        await invoke('open_system_settings', { preferencePane: 'Privacy_Microphone' });
      } catch (error) {
        console.error('Failed to open microphone settings:', error);
      }
    }
  };

  const openScreenRecordingSettings = async () => {
    if (isMacOS) {
      try {
        await invoke('open_system_settings', { preferencePane: 'Privacy_ScreenCapture' });
      } catch (error) {
        console.error('Failed to open screen recording settings:', error);
      }
    }
  };

  return (
    <div className="max-w-md mb-4 space-y-3">
      {/* Combined Permission Warning - Show when either permission is missing */}
      {(!hasMicrophone || !hasSystemAudio) && (
        <Alert variant="destructive" className="border-amber-400 bg-amber-50">
          <AlertTriangle className="h-5 w-5 text-amber-600" />
          <AlertTitle className="text-amber-900 font-semibold">
            <div className="flex items-center gap-2">
              {!hasMicrophone && <Mic className="h-4 w-4" />}
              {!hasSystemAudio && <Speaker className="h-4 w-4" />}
              {!hasMicrophone && !hasSystemAudio ? '권한이 필요합니다' : !hasMicrophone ? '마이크 권한이 필요합니다' : '시스템 오디오 권한이 필요합니다'}
            </div>
          </AlertTitle>
          {/* Action Buttons */}
          <div className="mt-4 flex flex-wrap gap-2">
            {isMacOS && !hasMicrophone && (
              <button
                onClick={openMicrophoneSettings}
                className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-amber-600 hover:bg-amber-700 rounded-md transition-colors"
              >
                <Mic className="h-4 w-4" />
                마이크 설정 열기
              </button>
            )}
            {isMacOS && !hasSystemAudio && (
              <button
                onClick={openScreenRecordingSettings}
                className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
              >
                <Speaker className="h-4 w-4" />
                화면 녹화 설정 열기
              </button>
            )}
            <button
              onClick={onRecheck}
              disabled={isRechecking}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-amber-900 bg-amber-100 hover:bg-amber-200 rounded-md transition-colors disabled:opacity-50"
            >
              <RefreshCw className={`h-4 w-4 ${isRechecking ? 'animate-spin' : ''}`} />
              다시 확인
            </button>
          </div>
          <AlertDescription className="text-amber-800 mt-2">
            {/* Microphone Warning */}
            {!hasMicrophone && (
              <>
                <p className="mb-3">
                  회의를 녹음하려면 Meetily에 마이크 접근 권한이 필요합니다. 마이크 장치가 감지되지 않았습니다.
                </p>
                <div className="space-y-2 text-sm mb-4">
                  <p className="font-medium">다음을 확인해 주세요.</p>
                  <ul className="list-disc list-inside ml-2 space-y-1">
                    <li>마이크가 연결되어 있고 전원이 켜져 있는지</li>
                    <li>시스템 설정에서 마이크 권한이 허용되어 있는지</li>
                    <li>다른 앱이 마이크를 독점 사용 중이지는 않은지</li>
                  </ul>
                </div>
              </>
            )}

            {/* System Audio Warning */}
            {!hasSystemAudio && (
              <>
                <p className="mb-3">
                  {hasMicrophone
                    ? '시스템 오디오 캡처를 사용할 수 없습니다. 마이크로는 계속 녹음할 수 있지만 컴퓨터 오디오는 녹음되지 않습니다.'
                    : '시스템 오디오 캡처도 사용할 수 없습니다.'}
                </p>
                {isMacOS && (
                  <div className="space-y-2 text-sm mb-4">
                    <p className="font-medium">macOS에서 시스템 오디오를 사용하려면:</p>
                    <ul className="list-disc list-inside ml-2 space-y-1">
                      <li>가상 오디오 장치를 설치하세요 (예: BlackHole 2ch)</li>
                      <li>Meetily에 화면 녹화 권한을 부여하세요</li>
                      <li>Audio MIDI 설정에서 오디오 라우팅을 구성하세요</li>
                    </ul>
                  </div>
                )}
              </>
            )}


          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}
