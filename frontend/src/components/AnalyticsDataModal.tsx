'use client';

import React from 'react';
import { X, Info, Shield } from 'lucide-react';

interface AnalyticsDataModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirmDisable: () => void;
}

export default function AnalyticsDataModal({ isOpen, onClose, onConfirmDisable }: AnalyticsDataModalProps) {
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200">
          <div className="flex items-center gap-3">
            <Shield className="w-6 h-6 text-blue-600" />
            <h2 className="text-xl font-semibold text-gray-900">수집하는 정보</h2>
          </div>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-6">
          {/* Privacy Notice */}
          <div className="bg-green-50 border border-green-200 rounded-lg p-4">
            <div className="flex items-start gap-3">
              <Info className="w-5 h-5 text-green-600 mt-0.5 flex-shrink-0" />
              <div className="text-sm text-green-800">
                <p className="font-semibold mb-1">개인정보는 안전하게 보호됩니다</p>
                <p><strong>익명 사용 데이터</strong>만 수집합니다. 회의 내용, 이름, 개인정보는 절대 수집하지 않습니다.</p>
              </div>
            </div>
          </div>

          {/* Data Categories */}
          <div className="space-y-4">
            <h3 className="text-lg font-semibold text-gray-900">수집하는 데이터:</h3>

            {/* Model Preferences */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">1. 모델 선호도</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>• Transcription model (e.g., "Cohere Transcribe 03-2026")</li>
                <li>• Summary model (e.g., "Llama 3.2", "Claude Sonnet")</li>
                <li>• Model provider (e.g., "Local", "Ollama", "OpenRouter")</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">사용자가 선호하는 모델을 파악하는 데 도움이 됩니다</p>
            </div>

            {/* Meeting Metrics */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">2. 익명 회의 지표</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>• Recording duration (e.g., "125 seconds")</li>
                <li>• Pause duration (e.g., "5 seconds")</li>
                <li>• Number of transcript segments</li>
                <li>• Number of audio chunks processed</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">성능을 최적화하고 사용 패턴을 파악하는 데 도움이 됩니다</p>
            </div>

            {/* Device Types */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">3. 기기 종류 (기기 이름 제외)</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>• Microphone type: "Bluetooth" or "Wired" or "Unknown"</li>
                <li>• System audio type: "Bluetooth" or "Wired" or "Unknown"</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">실제 기기 이름이 아니라 호환성을 개선하는 데 사용됩니다</p>
            </div>

            {/* Usage Patterns */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">4. 앱 사용 패턴</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>• App started/stopped events</li>
                <li>• Session duration</li>
                <li>• Feature usage (e.g., "settings changed")</li>
                <li>• Error occurrences (helps us fix bugs)</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">사용자 경험을 개선하는 데 도움이 됩니다</p>
            </div>

            {/* Platform Info */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">5. 플랫폼 정보</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>• Operating system (e.g., "macOS", "Windows")</li>
                <li>• App version (automatically included in all events)</li>
                <li>• Architecture (e.g., "x86_64", "aarch64")</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">플랫폼 지원 우선순위를 정하는 데 도움이 됩니다</p>
            </div>
          </div>

          {/* What We DON'T Collect */}
          <div className="bg-red-50 border border-red-200 rounded-lg p-4">
            <h4 className="font-semibold text-red-900 mb-2">수집하지 않는 정보:</h4>
            <ul className="text-sm text-red-800 space-y-1 ml-4">
              <li>• ❌ 회의 이름 또는 제목</li>
              <li>• ❌ 회의 전사본 또는 내용</li>
              <li>• ❌ 오디오 녹음 파일</li>
              <li>• ❌ 기기 이름 (종류만 수집: Bluetooth/유선)</li>
              <li>• ❌ 개인정보</li>
              <li>• ❌ 식별 가능한 모든 데이터</li>
            </ul>
          </div>

          {/* Example Event */}
          <div className="bg-gray-50 border border-gray-200 rounded-lg p-4">
            <h4 className="font-semibold text-gray-900 mb-2">이벤트 예시:</h4>
            <pre className="text-xs text-gray-700 overflow-x-auto">
              {`{
  "event": "meeting_ended",
  "app_version": "0.3.0",
  "transcription_provider": "cohere",
  "transcription_model": "cohere-transcribe-03-2026",
  "summary_provider": "ollama",
  "summary_model": "llama3.2:latest",
  "total_duration_seconds": "125.5",
  "microphone_device_type": "Wired",
  "system_audio_device_type": "Bluetooth",
  "chunks_processed": "150",
  "had_fatal_error": "false"
}`}
            </pre>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-4 p-6 border-t border-gray-200 bg-gray-50">
          <button
            onClick={onClose}
            className="px-4 py-2 text-gray-700 bg-white border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
          >
            애널리틱스 계속 사용
          </button>
          <button
            onClick={onConfirmDisable}
            className="px-4 py-2 text-white bg-red-600 rounded-md hover:bg-red-700 transition-colors"
          >
            확인: 애널리틱스 사용 중지
          </button>
        </div>
      </div>
    </div>
  );
}
