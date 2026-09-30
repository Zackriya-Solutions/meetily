'use client';
import { uiText, useUiTranslation } from '@/i18n/ui';

import React from 'react';
import { X, Info, Shield } from 'lucide-react';

interface AnalyticsDataModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirmDisable: () => void;
}

export default function AnalyticsDataModal({ isOpen, onClose, onConfirmDisable }: AnalyticsDataModalProps) {
  useUiTranslation();
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200">
          <div className="flex items-center gap-3">
            <Shield className="w-6 h-6 text-blue-600" />
            <h2 className="text-xl font-semibold text-gray-900">{uiText("messages.whatAnalyticsCollects")}</h2>
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
                <p className="font-semibold mb-1">{uiText("messages.yourPrivacyIsProtected")}</p>
                <p>{uiText("messages.analyticsIsOffByDefaultIfYouEnableIt")} <strong>{uiText("messages.anonymousUsageDataOnly")}</strong>{uiText("messages.noMeetingContentNamesFilePathsOrPersonalInformation")}</p>
              </div>
            </div>
          </div>

          {/* Data Categories */}
          <div className="space-y-4">
            <h3 className="text-lg font-semibold text-gray-900">{uiText("messages.dataWeCollectWhenEnabled")}</h3>

            {/* Model Preferences */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.1ModelPreferences")}</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>{uiText("messages.transcriptionModelEGWhisperLargeV3Parakeet")}</li>
                <li>{uiText("messages.summaryModelEGLlama32ClaudeSonnet")}</li>
                <li>{uiText("messages.modelProviderEGLocalOllamaOpenRouter")}</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">{uiText("messages.helpsUsUnderstandWhichModelsUsersPrefer")}</p>
            </div>

            {/* Meeting Metrics */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.2AnonymousMeetingMetrics")}</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>{uiText("messages.recordingDurationEG125Seconds")}</li>
                <li>{uiText("messages.pauseDurationEG5Seconds")}</li>
                <li>{uiText("messages.numberOfTranscriptSegments")}</li>
                <li>{uiText("messages.numberOfAudioChunksProcessed")}</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">{uiText("messages.helpsUsOptimizePerformanceAndUnderstandUsagePatterns")}</p>
            </div>

            {/* Device Types */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.3DeviceTypesNotNames")}</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>{uiText("messages.microphoneTypeBluetoothOrWiredOrUnknown")}</li>
                <li>{uiText("messages.systemAudioTypeBluetoothOrWiredOrUnknown")}</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">{uiText("messages.helpsUsImproveCompatibilityNOTTheActualDeviceNames")}</p>
            </div>

            {/* Usage Patterns */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.4AppUsagePatterns")}</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>{uiText("messages.appStartedStoppedEvents")}</li>
                <li>{uiText("messages.sessionDuration")}</li>
                <li>{uiText("messages.featureUsageEGSettingsChanged")}</li>
                <li>{uiText("messages.errorOccurrencesHelpsUsFixBugs")}</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">{uiText("messages.helpsUsImproveUserExperience")}</p>
            </div>

            {/* Platform Info */}
            <div className="border border-gray-200 rounded-lg p-4">
              <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.5PlatformInformation")}</h4>
              <ul className="text-sm text-gray-700 space-y-1 ml-4">
                <li>{uiText("messages.operatingSystemEGMacOSWindows")}</li>
                <li>{uiText("messages.appVersionAutomaticallyIncludedInAllEvents")}</li>
                <li>{uiText("messages.architectureEGX8664Aarch64")}</li>
              </ul>
              <p className="text-xs text-gray-500 mt-2 italic">{uiText("messages.helpsUsPrioritizePlatformSupport")}</p>
            </div>
          </div>

          {/* What We DON'T Collect */}
          <div className="bg-red-50 border border-red-200 rounded-lg p-4">
            <h4 className="font-semibold text-red-900 mb-2">{uiText("messages.whatWeDONTCollect")}</h4>
            <ul className="text-sm text-red-800 space-y-1 ml-4">
              <li>{uiText("messages.meetingNamesOrTitles")}</li>
              <li>{uiText("messages.fileNamesFilePathsOrMeetingFolders")}</li>
              <li>{uiText("messages.meetingTranscriptsOrContent")}</li>
              <li>{uiText("messages.audioRecordings")}</li>
              <li>{uiText("messages.deviceNamesOnlyTypesBluetoothWired")}</li>
              <li>{uiText("messages.personalInformation")}</li>
              <li>{uiText("messages.anyIdentifiableData")}</li>
            </ul>
          </div>

          {/* Example Event */}
          <div className="bg-gray-50 border border-gray-200 rounded-lg p-4">
            <h4 className="font-semibold text-gray-900 mb-2">{uiText("messages.exampleEvent")}</h4>
            <pre className="text-xs text-gray-700 overflow-x-auto">
              {`{
  "event": "meeting_ended",
  "app_version": "0.4.1",
  "transcription_provider": "parakeet",
  "transcription_model": "parakeet-tdt-0.6b-v3-int8",
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
             {uiText("messages.keepAnalyticsEnabled")} </button>
          <button
            onClick={onConfirmDisable}
            className="px-4 py-2 text-white bg-red-600 rounded-md hover:bg-red-700 transition-colors"
          >
             {uiText("messages.confirmDisableAnalytics")} </button>
        </div>
      </div>
    </div>
  );
}
