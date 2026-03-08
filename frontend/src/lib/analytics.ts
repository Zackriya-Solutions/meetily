import { invoke } from '@tauri-apps/api/core';

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

export class Analytics {
  private static initialized = false;
  private static currentUserId: string | null = null;
  private static initializationPromise: Promise<void> | null = null;
  private static sessionStartTime: number | null = null;
  private static meetingsInSession: number = 0;
  private static deviceInfo: DeviceInfo | null = null;
  private static currentSessionId: string | null = null;

  static async init(): Promise<void> {
    // Prevent duplicate initialization
    if (this.initialized) {
      return;
    }

    // If already initializing, wait for it to complete
    if (this.initializationPromise) {
      return this.initializationPromise;
    }

    this.initializationPromise = this.doInit();
    return this.initializationPromise;
  }

  private static async doInit(): Promise<void> {
    try {
      await invoke('init_analytics');
      this.initialized = true;
      console.log('Analytics initialized successfully');
    } catch (error) {
      console.error('Failed to initialize analytics:', error);
      throw error;
    } finally {
      this.initializationPromise = null;
    }
  }

  static async disable(): Promise<void> {
    try {
      await invoke('disable_analytics');
      this.initialized = false;
      this.currentUserId = null;
      this.initializationPromise = null;
      console.log('Analytics disabled successfully');
    } catch (error) {
      console.error('Failed to disable analytics:', error);
    }
  }

  static async isEnabled(): Promise<boolean> {
    try {
      return await invoke('is_analytics_enabled');
    } catch (error) {
      console.error('Failed to check analytics status:', error);
      return false;
    }
  }

  static async track(eventName: string, properties?: AnalyticsProperties): Promise<void> {
    if (!this.initialized) {
      console.warn('Analytics not initialized');
      return;
    }

    try {
      await invoke('track_event', { eventName, properties });
    } catch (error) {
      console.error(`Failed to track event ${eventName}:`, error);
    }
  }

  static async identify(userId: string, properties?: AnalyticsProperties): Promise<void> {
    if (!this.initialized) {
      console.warn('Analytics not initialized');
      return;
    }

    try {
      await invoke('identify_user', { userId, properties });
      this.currentUserId = userId;
    } catch (error) {
      console.error(`Failed to identify user ${userId}:`, error);
    }
  }

  // Enhanced user tracking methods for Phase 1
  static async startSession(userId: string): Promise<string | null> {
    if (!this.initialized) {
      console.warn('Analytics not initialized');
      return null;
    }

    try {
      const sessionId = await invoke('start_analytics_session', { userId });
      this.currentUserId = userId;
      
      return sessionId as string;
    } catch (error) {
      console.error('Failed to start analytics session:', error);
      return null;
    }
  }

  static async endSession(): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('end_analytics_session');
    } catch (error) {
      console.error('Failed to end analytics session:', error);
    }
  }

  static async trackDailyActiveUser(): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_daily_active_user');
    } catch (error) {
      console.error('Failed to track daily active user:', error);
    }
  }

  static async trackUserFirstLaunch(): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_user_first_launch');
    } catch (error) {
      console.error('Failed to track user first launch:', error);
    }
  }

  static async isSessionActive(): Promise<boolean> {
    if (!this.initialized) return false;

    try {
      return await invoke('is_analytics_session_active');
    } catch (error) {
      console.error('Failed to check session status:', error);
      return false;
    }
  }

  // User ID management with persistent storage
  static async getPersistentUserId(): Promise<string> {
    try {
      // First check if we have a stored user ID
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      
      let userId = await store.get<string>('user_id');
      
      if (!userId) {
        // Generate new user ID
        userId = `user_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
        await store.set('user_id', userId);
        await store.set('is_first_launch', true);
        await store.save();
      }
      
      return userId;
    } catch (error) {
      console.error('Failed to get persistent user ID:', error);
      // Fallback to session storage
      let userId = sessionStorage.getItem('clearminutes_user_id');
      if (!userId) {
        userId = `user_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
        sessionStorage.setItem('clearminutes_user_id', userId);
        sessionStorage.setItem('is_first_launch', 'true');
      }
      return userId;
    }
  }

  static async checkAndTrackFirstLaunch(): Promise<void> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      
      const isFirstLaunch = await store.get<boolean>('is_first_launch');
      
      if (isFirstLaunch) {
        await this.trackUserFirstLaunch();
        await store.set('is_first_launch', false);
        await store.save();
      }
    } catch (error) {
      console.error('Failed to check first launch:', error);
      // Fallback to session storage
      const isFirstLaunch = sessionStorage.getItem('is_first_launch') === 'true';
      if (isFirstLaunch) {
        await this.trackUserFirstLaunch();
        sessionStorage.removeItem('is_first_launch');
      }
    }
  }

  static async checkAndTrackDailyUsage(): Promise<void> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      
      const today = new Date().toISOString().split('T')[0];
      const lastTrackedDate = await store.get<string>('last_daily_tracked');
      
      if (lastTrackedDate !== today) {
        await this.trackDailyActiveUser();
        await store.set('last_daily_tracked', today);
        await store.save();
      }
    } catch (error) {
      console.error('Failed to check daily usage:', error);
    }
  }

  static getCurrentUserId(): string | null {
    return this.currentUserId;
  }

  // Platform/Device detection methods
  static async getPlatform(): Promise<string> {
    try {
      // Use browser's user agent as fallback
      const userAgent = navigator.userAgent.toLowerCase();
      if (userAgent.includes('mac')) return 'macOS';
      if (userAgent.includes('win')) return 'Windows';
      if (userAgent.includes('linux')) return 'Linux';
      return 'unknown';
    } catch (error) {
      console.error('Failed to get platform:', error);
      return 'unknown';
    }
  }

  static async getOSVersion(): Promise<string> {
    try {
      const platform = await this.getPlatform();
      // Use navigator.userAgent for version info
      const userAgent = navigator.userAgent;
      return `${platform} (${userAgent})`;
    } catch (error) {
      console.error('Failed to get OS version:', error);
      return 'unknown';
    }
  }

  static async getDeviceInfo(): Promise<DeviceInfo> {
    if (this.deviceInfo) return this.deviceInfo;

    try {
      const platform = await this.getPlatform();
      const osVersion = await this.getOSVersion();

      // Detect architecture from user agent
      const userAgent = navigator.userAgent.toLowerCase();
      let architecture = 'unknown';
      if (userAgent.includes('arm') || userAgent.includes('aarch64')) {
        architecture = 'aarch64';
      } else if (userAgent.includes('x86_64') || userAgent.includes('x64')) {
        architecture = 'x86_64';
      } else if (userAgent.includes('x86')) {
        architecture = 'x86';
      }

      this.deviceInfo = {
        platform: platform,
        os_version: osVersion,
        architecture: architecture
      };

      return this.deviceInfo;
    } catch (error) {
      console.error('Failed to get device info:', error);
      return {
        platform: 'unknown',
        os_version: 'unknown',
        architecture: 'unknown'
      };
    }
  }

  // Shared helper — returns device info + browser language + session ID merged with any extra properties
  private static async enrichProperties(extra?: AnalyticsProperties): Promise<AnalyticsProperties> {
    const deviceInfo = await this.getDeviceInfo();
    return {
      platform: deviceInfo.platform,
      os_version: deviceInfo.os_version,
      architecture: deviceInfo.architecture,
      $browser_language: navigator.language,
      ...(this.currentSessionId ? { $session_id: this.currentSessionId } : {}),
      ...extra,
    };
  }

  // Helper methods for analytics.json store
  static async calculateDaysSince(dateKey: string): Promise<number | null> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const dateStr = await store.get<string>(dateKey);
      if (!dateStr) return null;
      const diffMs = Date.now() - new Date(dateStr).getTime();
      return Math.floor(diffMs / (1000 * 60 * 60 * 24));
    } catch (error) {
      console.error(`Failed to calculate days since ${dateKey}:`, error);
      return null;
    }
  }

  static async updateMeetingCount(): Promise<void> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');

      const totalMeetings = (await store.get<number>('total_meetings') || 0) + 1;
      await store.set('total_meetings', totalMeetings);
      await store.set('last_meeting_date', new Date().toISOString());

      // Update daily count
      const today = new Date().toISOString().split('T')[0];
      const dailyCounts = await store.get<Record<string, number>>('daily_meeting_counts') || {};
      dailyCounts[today] = (dailyCounts[today] || 0) + 1;
      await store.set('daily_meeting_counts', dailyCounts);
      await store.save();
    } catch (error) {
      console.error('Failed to update meeting count:', error);
    }
  }

  static async getMeetingsCountToday(): Promise<number> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const today = new Date().toISOString().split('T')[0];
      const dailyCounts = await store.get<Record<string, number>>('daily_meeting_counts') || {};
      return dailyCounts[today] || 0;
    } catch (error) {
      console.error('Failed to get meetings count today:', error);
      return 0;
    }
  }

  static async hasUsedFeatureBefore(featureName: string): Promise<boolean> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const features = await store.get<Record<string, any>>('features_used') || {};
      return !!features[featureName];
    } catch (error) {
      console.error(`Failed to check feature usage for ${featureName}:`, error);
      return false;
    }
  }

  static async markFeatureUsed(featureName: string): Promise<void> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const features = await store.get<Record<string, any>>('features_used') || {};

      if (!features[featureName]) {
        features[featureName] = {
          first_used: new Date().toISOString(),
          use_count: 1
        };
      } else {
        features[featureName].use_count++;
      }

      await store.set('features_used', features);
      await store.save();
    } catch (error) {
      console.error(`Failed to mark feature used for ${featureName}:`, error);
    }
  }

  // Enhanced session tracking with platform info
  private static heartbeatInterval: ReturnType<typeof setInterval> | null = null;

  static async trackSessionStarted(sessionId: string): Promise<void> {
    if (!this.initialized) return;

    this.currentSessionId = sessionId;
    this.sessionStartTime = Date.now();
    this.meetingsInSession = 0;

    try {
      const daysSinceLast = await this.calculateDaysSince('last_meeting_date');
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const totalMeetings = await store.get<number>('total_meetings') || 0;

      await this.track('session_started', await this.enrichProperties({
        session_id: sessionId,
        days_since_last_meeting: daysSinceLast?.toString() ?? 'null',
        total_meetings: totalMeetings.toString(),
      }));

      this.startSessionHeartbeat(sessionId);
    } catch (error) {
      console.error('Failed to track session started:', error);
    }
  }

  static async trackSessionEnded(sessionId: string): Promise<void> {
    if (!this.initialized || !this.sessionStartTime) return;

    this.stopSessionHeartbeat();

    try {
      const sessionDuration = (Date.now() - this.sessionStartTime) / 1000;

      await this.track('session_ended', await this.enrichProperties({
        session_id: sessionId,
        session_duration_seconds: sessionDuration.toFixed(0),
        meetings_in_session: this.meetingsInSession.toString(),
      }));
    } catch (error) {
      console.error('Failed to track session ended:', error);
    } finally {
      this.currentSessionId = null;
      this.sessionStartTime = null;
    }
  }

  // Sends a heartbeat every 5 minutes so PostHog can accurately measure session length
  // even if beforeunload doesn't fire (common in Tauri)
  private static startSessionHeartbeat(sessionId: string): void {
    this.stopSessionHeartbeat();
    this.heartbeatInterval = setInterval(async () => {
      if (!this.initialized || !this.sessionStartTime) return;
      try {
        const elapsed = ((Date.now() - this.sessionStartTime) / 1000).toFixed(0);
        await this.track('session_heartbeat', await this.enrichProperties({
          session_id: sessionId,
          elapsed_seconds: elapsed,
          meetings_in_session: this.meetingsInSession.toString(),
        }));
      } catch { /* silent — heartbeat is best-effort */ }
    }, 5 * 60 * 1000);
  }

  private static stopSessionHeartbeat(): void {
    if (this.heartbeatInterval !== null) {
      clearInterval(this.heartbeatInterval);
      this.heartbeatInterval = null;
    }
  }

  // Enhanced meeting completion tracking
  static async trackMeetingCompleted(meetingId: string, metrics: {
    duration_seconds: number;
    transcript_segments: number;
    transcript_word_count: number;
    words_per_minute: number;
    meetings_today: number;
  }): Promise<void> {
    if (!this.initialized) return;

    try {
      const deviceInfo = await this.getDeviceInfo();

      await this.track('meeting_completed', {
        meeting_id: meetingId,
        duration_seconds: metrics.duration_seconds.toString(),
        transcript_segments: metrics.transcript_segments.toString(),
        transcript_word_count: metrics.transcript_word_count.toString(),
        words_per_minute: metrics.words_per_minute.toFixed(2),
        meetings_today: metrics.meetings_today.toString(),
        day_of_week: new Date().getDay().toString(),
        hour_of_day: new Date().getHours().toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      });

      this.meetingsInSession++;
    } catch (error) {
      console.error('Failed to track meeting completed:', error);
    }
  }

  // Feature usage tracking with platform info
  static async trackFeatureUsedEnhanced(featureName: string, properties?: Record<string, any>): Promise<void> {
    if (!this.initialized) return;

    try {
      const deviceInfo = await this.getDeviceInfo();
      const isFirstUse = !(await this.hasUsedFeatureBefore(featureName));
      await this.markFeatureUsed(featureName);

      const trackingProperties: AnalyticsProperties = {
        feature_name: featureName,
        is_first_use: isFirstUse.toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      };

      // Add additional properties if provided
      if (properties) {
        Object.entries(properties).forEach(([key, value]) => {
          trackingProperties[key] = String(value);
        });
      }

      await this.track('feature_used', trackingProperties);
    } catch (error) {
      console.error(`Failed to track feature used: ${featureName}`, error);
    }
  }

  // Copy tracking with frequency
  static async trackCopy(copyType: 'transcript' | 'summary', properties?: Record<string, any>): Promise<void> {
    if (!this.initialized) return;

    try {
      const deviceInfo = await this.getDeviceInfo();
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');

      // Get today's date
      const today = new Date().toISOString().split('T')[0];
      const copyCounts = await store.get<Record<string, any>>('copy_counts') || {};
      const todayCounts = copyCounts[today] || {};
      const copyCount = todayCounts[copyType] || 0;

      // Update copy count
      todayCounts[copyType] = copyCount + 1;
      copyCounts[today] = todayCounts;
      await store.set('copy_counts', copyCounts);
      await store.save();

      const trackingProperties: AnalyticsProperties = {
        copy_type: copyType,
        copy_count_today: (copyCount + 1).toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      };

      // Add additional properties if provided
      if (properties) {
        Object.entries(properties).forEach(([key, value]) => {
          trackingProperties[key] = String(value);
        });
      }

      await this.track(`${copyType}_copied`, trackingProperties);
    } catch (error) {
      console.error(`Failed to track ${copyType} copy:`, error);
    }
  }

  // Meeting-specific tracking methods
  static async trackMeetingStarted(meetingId: string, meetingTitle: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_meeting_started', { meetingId, meetingTitle });
    } catch (error) {
      console.error('Failed to track meeting started:', error);
    }
  }

  static async trackRecordingStarted(meetingId: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_recording_started', { meetingId });
    } catch (error) {
      console.error('Failed to track recording started:', error);
    }
  }

  static async trackRecordingStopped(meetingId: string, durationSeconds?: number): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_recording_stopped', { meetingId, durationSeconds });
    } catch (error) {
      console.error('Failed to track recording stopped:', error);
    }
  }

  static async trackMeetingDeleted(meetingId: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_meeting_deleted', { meetingId });
    } catch (error) {
      console.error('Failed to track meeting deleted:', error);
    }
  }

  static async trackSettingsChanged(settingType: string, newValue: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_settings_changed', { settingType, newValue });
    } catch (error) {
      console.error('Failed to track settings changed:', error);
    }
  }

  static async trackFeatureUsed(featureName: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_feature_used', { featureName });
    } catch (error) {
      console.error('Failed to track feature used:', error);
    }
  }

  // Error tracking — sends a $exception event to PostHog's Error Tracking UI
  static async captureException(
    error: unknown,
    context?: { handled?: boolean } & AnalyticsProperties
  ): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const { handled = true, ...extraContext } = context ?? {};
      const exceptionType = error instanceof Error ? error.constructor.name : 'UnknownError';
      const message = error instanceof Error ? error.message : String(error);

      await invoke('capture_exception', {
        exceptionType,
        message,
        handled,
        context: Object.keys(extraContext).length > 0 ? extraContext : null,
      });
    } catch (e) {
      console.error('Failed to capture exception:', e);
    }
  }

  // Integration tracking
  static async trackIntegrationUsed(
    integrationName: string,
    success: boolean,
    extra?: AnalyticsProperties
  ): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      await this.track('integration_used', await this.enrichProperties({
        integration: integrationName,
        success: success.toString(),
        ...extra,
      }));
    } catch (error) {
      console.error(`Failed to track integration_used (${integrationName}):`, error);
    }
  }

  // Convenience methods for common events
  static async trackPageView(pageName: string): Promise<void> {
    // Wait for analytics to be ready — page views often fire before init completes
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    await this.track('$pageview', await this.enrichProperties({
      page: pageName,
      $current_url: `app://clearminutes/${pageName}`,
      $pathname: `/${pageName}`,
      $host: 'clearminutes',
    }));
  }

  static async trackButtonClick(buttonName: string, location?: string): Promise<void> {
    const properties: AnalyticsProperties = { button: buttonName };
    if (location) properties.location = location;
    await this.track(`button_click_${buttonName}`, properties);
  }

  static async trackError(errorType: string, errorMessage: string): Promise<void> {
    await this.track('error', { 
      error_type: errorType, 
      error_message: errorMessage 
    });
  }

  static async trackAppStarted(): Promise<void> {
    try {
      await this.track('app_started', await this.enrichProperties({
        timestamp: new Date().toISOString(),
      }));
    } catch (error) {
      console.error('Failed to track app started:', error);
    }
  }

  static async trackAppClosed(): Promise<void> {
    try {
      const uptimeSeconds = this.sessionStartTime
        ? ((Date.now() - this.sessionStartTime) / 1000).toFixed(0)
        : 'unknown';
      await this.track('app_closed', await this.enrichProperties({
        timestamp: new Date().toISOString(),
        uptime_seconds: uptimeSeconds,
      }));
    } catch (error) {
      console.error('Failed to track app closed:', error);
    }
  }

  // Cleanup method for app shutdown
  static async cleanup(): Promise<void> {
    await this.endSession();
  }

  // Reset initialization state (useful for testing)
  static reset(): void {
    this.stopSessionHeartbeat();
    this.initialized = false;
    this.currentUserId = null;
    this.currentSessionId = null;
    this.sessionStartTime = null;
    this.initializationPromise = null;
  }

  // Wait for analytics to be initialized
  static async waitForInitialization(timeout: number = 5000): Promise<boolean> {
    if (this.initialized) {
      return true;
    }
    
    const startTime = Date.now();
    while (!this.initialized && (Date.now() - startTime) < timeout) {
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    
    return this.initialized;
  }

  // Track backend connection success/failure
  static async trackBackendConnection(success: boolean, error?: string) {
    // Wait for analytics to be initialized
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) {
      console.warn('Analytics not initialized within timeout, skipping backend connection tracking');
      return;
    }

    try {
      console.log('Tracking backend connection event:', { success, error });
      await invoke('track_event', {
        eventName: 'backend_connection',
        properties: {
          success: success.toString(),
          error: error || '',
          timestamp: new Date().toISOString()
        }
      });
      console.log('Backend connection event tracked successfully');
    } catch (error) {
      console.error('Failed to track backend connection:', error);
    }
  }

  // Track transcription errors
  static async trackTranscriptionError(errorMessage: string) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping transcription error tracking');
      return;
    }

    try {
      console.log('Tracking transcription error event:', { errorMessage });
      await invoke('track_event', {
        eventName: 'transcription_error',
        properties: {
          error_message: errorMessage,
          timestamp: new Date().toISOString()
        }
      });
      console.log('Transcription error event tracked successfully');
    } catch (error) {
      console.error('Failed to track transcription error:', error);
    }
  }

  // Track transcription success
  static async trackTranscriptionSuccess(duration?: number) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping transcription success tracking');
      return;
    }

    try {
      console.log('Tracking transcription success event:', { duration });
      await invoke('track_event', {
        eventName: 'transcription_success',
        properties: {
          duration: duration ? duration.toString() : '',
          timestamp: new Date().toISOString()
        }
      });
      console.log('Transcription success event tracked successfully');
    } catch (error) {
      console.error('Failed to track transcription success:', error);
    }
  }

  // Summary generation analytics
  static async trackSummaryGenerationStarted(
    modelProvider: string,
    modelName: string,
    transcriptLength: number,
    timeSinceRecordingMinutes?: number
  ) {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const props = await this.enrichProperties({
        model_provider: modelProvider,
        model_name: modelName,
        transcript_length: transcriptLength.toString(),
        ...(timeSinceRecordingMinutes !== undefined && {
          time_since_recording_minutes: timeSinceRecordingMinutes.toFixed(2),
        }),
      });
      await invoke('track_event', { eventName: 'summary_generation_started', properties: props });
    } catch (error) {
      console.error('Failed to track summary generation started:', error);
    }
  }

  static async trackSummaryGenerationCompleted(
    modelProvider: string,
    modelName: string,
    success: boolean,
    durationSeconds?: number,
    errorMessage?: string
  ) {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const props = await this.enrichProperties({
        model_provider: modelProvider,
        model_name: modelName,
        success: success.toString(),
        ...(durationSeconds !== undefined && { duration_seconds: durationSeconds.toString() }),
        ...(errorMessage && { error_message: errorMessage }),
      });
      await invoke('track_event', { eventName: 'summary_generation_completed', properties: props });
    } catch (error) {
      console.error('Failed to track summary generation completed:', error);
    }
  }

  static async trackSummaryRegenerated(modelProvider: string, modelName: string) {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const props = await this.enrichProperties({
        model_provider: modelProvider,
        model_name: modelName,
      });
      await invoke('track_event', { eventName: 'summary_regenerated', properties: props });
    } catch (error) {
      console.error('Failed to track summary regenerated:', error);
    }
  }

  static async trackModelChanged(oldProvider: string, oldModel: string, newProvider: string, newModel: string) {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const props = await this.enrichProperties({
        old_provider: oldProvider,
        old_model: oldModel,
        new_provider: newProvider,
        new_model: newModel,
      });
      await invoke('track_event', { eventName: 'model_changed', properties: props });
    } catch (error) {
      console.error('Failed to track model changed:', error);
    }
  }

  static async trackCustomPromptUsed(promptLength: number) {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      const props = await this.enrichProperties({
        prompt_length: promptLength.toString(),
      });
      await invoke('track_event', { eventName: 'custom_prompt_used', properties: props });
    } catch (error) {
      console.error('Failed to track custom prompt used:', error);
    }
  }

  // Onboarding analytics
  static async trackOnboardingCompleted(selectedSummaryModel: string): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      await this.track('onboarding_completed', await this.enrichProperties({
        selected_summary_model: selectedSummaryModel,
        timestamp: new Date().toISOString(),
      }));
    } catch (error) {
      console.error('Failed to track onboarding completed:', error);
    }
  }

  // App update analytics
  static async trackUpdateAvailable(version: string, currentVersion?: string): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      await this.track('update_available', await this.enrichProperties({
        new_version: version,
        current_version: currentVersion ?? 'unknown',
        timestamp: new Date().toISOString(),
      }));
    } catch (error) {
      console.error('Failed to track update available:', error);
    }
  }

  static async trackUpdateDownloadStarted(version: string): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      await this.track('update_download_started', await this.enrichProperties({
        version,
        timestamp: new Date().toISOString(),
      }));
    } catch (error) {
      console.error('Failed to track update download started:', error);
    }
  }

  static async trackUpdateInstalled(version: string): Promise<void> {
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) return;

    try {
      await this.track('update_installed', await this.enrichProperties({
        version,
        timestamp: new Date().toISOString(),
      }));
    } catch (error) {
      console.error('Failed to track update installed:', error);
    }
  }
}

export default Analytics;
