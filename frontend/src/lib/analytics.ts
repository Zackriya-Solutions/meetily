import { authFetch } from './api';

export interface AnalyticsProperties {
  [key: string]: any;
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

export class Analytics {
  private static initialized = false;
  private static currentUserId: string | null = null;
  private static currentSessionId: string | null = null;

  static async init(userId?: string): Promise<void> {
    console.log('[Analytics] Init');
    this.initialized = true;
    this.currentUserId = userId || await this.getPersistentUserId();
    this.currentSessionId = await this.startSession(this.currentUserId);
  }

  static async disable(): Promise<void> {
    console.log('[Analytics] Disable');
    this.initialized = false;
  }

  static async isEnabled(): Promise<boolean> {
    const storedOptIn = typeof localStorage !== 'undefined' ? localStorage.getItem('analyticsOptedIn') : null;
    return storedOptIn !== 'false' && this.initialized;
  }

  static async track(eventName: string, properties?: AnalyticsProperties): Promise<void> {
    console.log('[Analytics] Track:', eventName, properties);
    if (!(await this.isEnabled())) return;

    // Skip sending events if running on localhost development environment
    if (typeof window !== 'undefined' && 
        (window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1' || window.location.port === '3118')) {
      console.log('[Analytics - Localhost] Development mode detected. Skipping event upload:', eventName);
      return;
    }

    try {
      authFetch('/analytics/track', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          event_name: eventName,
          properties: properties || {},
          session_id: this.currentSessionId,
          user_id: this.currentUserId
        })
      }).catch(err => {
        // Silently fail if analytics backend is unreachable
        console.warn('[Analytics] Failed to send event:', err);
      });
    } catch (e) {
      // Catch synchronous errors just in case
    }
  }

  static async identify(userId: string, properties?: AnalyticsProperties): Promise<void> {
    console.log('[Analytics] Identify:', userId, properties);
    this.currentUserId = userId;
  }

  static async startSession(userId: string): Promise<string | null> {
    const sessionId = 'web-session-' + Date.now();
    this.currentSessionId = sessionId;
    return sessionId;
  }

  static async endSession(): Promise<void> {
    console.log('[Analytics] End Session');
  }

  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  
  static async isSessionActive(): Promise<boolean> {
    return true;
  }

  static async getPersistentUserId(): Promise<string> {
    let userId = typeof sessionStorage !== 'undefined' ? sessionStorage.getItem('meeting_copilot_user_id') : null;
    if (!userId) {
      userId = `user_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
      if (typeof sessionStorage !== 'undefined') sessionStorage.setItem('meeting_copilot_user_id', userId);
    }
    return userId;
  }

  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}

  static getCurrentUserId(): string | null {
    return this.currentUserId;
  }

  static async getPlatform(): Promise<string> {
    return 'Web';
  }

  static async getOSVersion(): Promise<string> {
    return 'Web';
  }

  static async getDeviceInfo(): Promise<DeviceInfo> {
    return {
      platform: 'Web',
      os_version: 'Web',
      architecture: 'unknown'
    };
  }

  static async calculateDaysSince(dateKey: string): Promise<number | null> {
    return 0;
  }

  static async updateMeetingCount(): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async hasUsedFeatureBefore(featureName: string): Promise<boolean> { return false; }
  static async markFeatureUsed(featureName: string): Promise<void> {}

  static async trackSessionStarted(sessionId: string): Promise<void> {}
  static async trackSessionEnded(sessionId: string): Promise<void> {}
  
  static async trackMeetingCompleted(meetingId: string, metrics: any): Promise<void> {
    this.track('meeting_completed', { meeting_id: meetingId, ...metrics });
  }

  static async trackFeatureUsedEnhanced(featureName: string, properties?: Record<string, any>): Promise<void> {
    this.track('feature_used', { feature: featureName, ...properties });
  }

  static async trackCopy(copyType: 'transcript' | 'summary', properties?: Record<string, any>): Promise<void> {
    this.track('content_copied', { type: copyType, ...properties });
  }

  static async trackMeetingStarted(meetingId: string, meetingTitle: string): Promise<void> {
    this.track('meeting_started', { meeting_id: meetingId, title_length: meetingTitle.length });
  }
  static async trackRecordingStarted(meetingId: string): Promise<void> {
    this.track('recording_started', { meeting_id: meetingId });
  }
  static async trackRecordingStopped(meetingId: string, durationSeconds?: number): Promise<void> {
    this.track('recording_stopped', { meeting_id: meetingId, duration: durationSeconds });
  }
  static async trackMeetingDeleted(meetingId: string): Promise<void> {
    this.track('meeting_deleted', { meeting_id: meetingId });
  }
  static async trackSettingsChanged(settingType: string, newValue: string): Promise<void> {
    this.track('settings_changed', { setting_type: settingType, new_value: newValue });
  }
  static async trackFeatureUsed(featureName: string): Promise<void> {
    this.track('feature_used', { feature: featureName });
  }
  
  static async trackPageView(pageName: string): Promise<void> {
    this.track('page_view', { page: pageName });
  }
  static async trackButtonClick(buttonName: string, location?: string): Promise<void> {
    this.track('button_click', { button: buttonName, location });
  }
  static async trackError(errorType: string, errorMessage: string): Promise<void> {
    this.track('error_occurred', { error_type: errorType, message: errorMessage });
  }
  static async trackAppStarted(): Promise<void> {
    this.track('app_started');
  }
  static async cleanup(): Promise<void> {}
  static reset(): void {}
  static async waitForInitialization(timeout: number = 5000): Promise<boolean> { return true; }
  static async trackBackendConnection(success: boolean, error?: string) {
    this.track('backend_connection', { success, error });
  }
  static async trackTranscriptionError(errorMessage: string) {
    this.track('transcription_error', { message: errorMessage });
  }
  static async trackTranscriptionSuccess(duration?: number) {
    this.track('transcription_success', { duration });
  }
  
  static async trackSummaryGenerationStarted(
    modelProvider: string,
    modelName: string,
    transcriptLength: number,
    timeSinceRecordingMinutes?: number
  ) {
    this.track('summary_generation_started', { 
      provider: modelProvider, 
      model: modelName, 
      transcript_length: transcriptLength 
    });
  }

  static async trackSummaryGenerationCompleted(
    modelProvider: string, 
    modelName: string, 
    success: boolean, 
    durationSeconds?: number, 
    errorMessage?: string
  ) {
    this.track('notes_generated', { 
      llm_model: `${modelProvider}_${modelName}`,
      success,
      duration: durationSeconds,
      error: errorMessage
    });
  }

  static async trackSummaryRegenerated(modelProvider: string, modelName: string) {
    this.track('notes_regenerated', { llm_model: `${modelProvider}_${modelName}` });
  }

  static async trackModelChanged(oldProvider: string, oldModel: string, newProvider: string, newModel: string) {
    this.track('model_changed', { old_model: oldModel, new_model: newModel });
  }

  static async trackCustomPromptUsed(length: number) {
    this.track('custom_prompt_used', { prompt_length: length });
  }
}

export default Analytics;