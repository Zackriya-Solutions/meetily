export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

export interface UserSession {
  session_id: string;
  user_id: string;
  start_time: string;
  last_heartbeat: string;
  is_active: boolean;
}

const DEFAULT_DEVICE_INFO: DeviceInfo = {
  platform: 'unknown',
  os_version: 'unknown',
  architecture: 'unknown',
};

export class Analytics {
  static async init(): Promise<void> {}
  static async disable(): Promise<void> {}
  static async isEnabled(): Promise<boolean> { return false; }
  static async track(_eventName: string, _properties?: AnalyticsProperties): Promise<void> {}
  static async identify(_userId: string, _properties?: AnalyticsProperties): Promise<void> {}
  static async startSession(_userId: string): Promise<string | null> { return null; }
  static async endSession(): Promise<void> {}
  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  static async isSessionActive(): Promise<boolean> { return false; }
  static async getPersistentUserId(): Promise<string> { return 'telemetry-disabled'; }
  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}
  static getCurrentUserId(): string | null { return null; }
  static async getPlatform(): Promise<string> { return DEFAULT_DEVICE_INFO.platform; }
  static async getOSVersion(): Promise<string> { return DEFAULT_DEVICE_INFO.os_version; }
  static async getArchitecture(): Promise<string> { return DEFAULT_DEVICE_INFO.architecture; }
  static async getDeviceInfo(): Promise<DeviceInfo> { return DEFAULT_DEVICE_INFO; }
  static async trackSessionStarted(_sessionId: string): Promise<void> {}
  static async trackSessionEnded(_sessionId: string): Promise<void> {}
  static async trackMeetingCompleted(
    _meetingId: string,
    _metrics: {
      duration_seconds: number;
      transcript_segments: number;
      transcript_word_count: number;
      words_per_minute: number;
      meetings_today: number;
    },
  ): Promise<void> {}
  static async trackFeatureUsedEnhanced(_featureName: string, _properties?: Record<string, unknown>): Promise<void> {}
  static async trackCopy(_copyType: 'transcript' | 'summary', _properties?: Record<string, unknown>): Promise<void> {}
  static async trackMeetingStarted(_meetingId: string, _meetingTitle: string): Promise<void> {}
  static async trackRecordingStarted(_meetingId: string): Promise<void> {}
  static async trackRecordingStopped(_meetingId: string, _durationSeconds?: number): Promise<void> {}
  static async trackMeetingDeleted(_meetingId: string): Promise<void> {}
  static async trackSettingsChanged(_settingType: string, _newValue: string): Promise<void> {}
  static async trackFeatureUsed(_featureName: string): Promise<void> {}
  static async trackPageView(_pageName: string): Promise<void> {}
  static async trackButtonClick(_buttonName: string, _location?: string): Promise<void> {}
  static async trackError(_errorType: string, _errorMessage: string): Promise<void> {}
  static async trackAppStarted(): Promise<void> {}
  static async cleanup(): Promise<void> {}
  static reset(): void {}
  static async waitForInitialization(_timeout = 5000): Promise<boolean> { return false; }
  static async trackBackendConnection(_success: boolean, _error?: string): Promise<void> {}
  static async trackTranscriptionError(_errorMessage: string): Promise<void> {}
  static async trackTranscriptionSuccess(_duration?: number): Promise<void> {}
  static async trackSummaryGenerationStarted(
    _modelProvider: string,
    _modelName: string,
    _transcriptLength: number,
    _timeSinceRecordingMinutes?: number,
  ): Promise<void> {}
  static async trackSummaryGenerationCompleted(
    _modelProvider: string,
    _modelName: string,
    _success: boolean,
    _durationSeconds?: number,
    _errorMessage?: string,
  ): Promise<void> {}
  static async trackSummaryRegenerated(_modelProvider: string, _modelName: string): Promise<void> {}
  static async trackModelChanged(
    _oldProvider: string,
    _oldModel: string,
    _newProvider: string,
    _newModel: string,
  ): Promise<void> {}
  static async trackCustomPromptUsed(_promptLength: number): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async updateMeetingCount(): Promise<void> {}
  static async calculateDaysSince(_key: string): Promise<number> { return 0; }
}

export default Analytics;
