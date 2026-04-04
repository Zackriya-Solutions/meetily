export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

// Analytics has been intentionally disabled for MeetFree v0.1.0.
// This shim preserves call sites without collecting or sending any data.
export class Analytics {
  private static currentUserId: string | null = null;

  static async init(): Promise<void> {}
  static async disable(): Promise<void> {}
  static async isEnabled(): Promise<boolean> { return false; }
  static async track(_eventName: string, _properties?: AnalyticsProperties): Promise<void> {}
  static async identify(userId: string, _properties?: AnalyticsProperties): Promise<void> { this.currentUserId = userId; }
  static async startSession(_userId: string): Promise<string | null> { return null; }
  static async endSession(): Promise<void> {}
  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  static async isSessionActive(): Promise<boolean> { return false; }
  static async getPersistentUserId(): Promise<string> {
    if (!this.currentUserId) {
      this.currentUserId = `user_${Date.now()}`;
    }
    return this.currentUserId;
  }
  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}
  static getCurrentUserId(): string | null { return this.currentUserId; }
  static async getPlatform(): Promise<string> { return "unknown"; }
  static async getOSVersion(): Promise<string> { return "unknown"; }
  static async getDeviceInfo(): Promise<DeviceInfo> {
    return { platform: "unknown", os_version: "unknown", architecture: "unknown" };
  }
  static async calculateDaysSince(_dateKey: string): Promise<number | null> { return null; }
  static async updateMeetingCount(): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async hasUsedFeatureBefore(_featureName: string): Promise<boolean> { return false; }
  static async markFeatureUsed(_featureName: string): Promise<void> {}
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
    }
  ): Promise<void> {}
  static async trackFeatureUsedEnhanced(_featureName: string, _properties?: Record<string, any>): Promise<void> {}
  static async trackCopy(_copyType: "transcript" | "summary", _properties?: Record<string, any>): Promise<void> {}
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
  static reset(): void { this.currentUserId = null; }
  static async waitForInitialization(_timeout: number = 5000): Promise<boolean> { return true; }
  static async trackBackendConnection(_success: boolean, _error?: string): Promise<void> {}
  static async trackTranscriptionError(_errorMessage: string): Promise<void> {}
  static async trackTranscriptionSuccess(_duration?: number): Promise<void> {}
  static async trackSummaryGenerationStarted(
    _modelProvider: string,
    _modelName: string,
    _transcriptLength: number,
    _timeSinceRecordingMinutes?: number
  ): Promise<void> {}
  static async trackSummaryGenerationCompleted(
    _modelProvider: string,
    _modelName: string,
    _success: boolean,
    _durationSeconds?: number,
    _errorMessage?: string
  ): Promise<void> {}
  static async trackSummaryRegenerated(_modelProvider: string, _modelName: string): Promise<void> {}
  static async trackModelChanged(
    _oldProvider: string,
    _oldModel: string,
    _newProvider: string,
    _newModel: string
  ): Promise<void> {}
  static async trackCustomPromptUsed(_promptLength: number): Promise<void> {}
}

export default Analytics;
