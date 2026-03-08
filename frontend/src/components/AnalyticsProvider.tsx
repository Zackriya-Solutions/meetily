'use client';

import React, { useEffect, ReactNode, useRef, useState, createContext } from 'react';
import Analytics from '@/lib/analytics';
import { load } from '@tauri-apps/plugin-store';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface AnalyticsProviderProps {
  children: ReactNode;
}

interface AnalyticsContextType {
  isAnalyticsOptedIn: boolean;
  setIsAnalyticsOptedIn: (optedIn: boolean) => void;
}

export const AnalyticsContext = createContext<AnalyticsContextType>({
  isAnalyticsOptedIn: true,
  setIsAnalyticsOptedIn: () => {},
});

export default function AnalyticsProvider({ children }: AnalyticsProviderProps) {
  const [isAnalyticsOptedIn, setIsAnalyticsOptedIn] = useState(true);
  const initialized = useRef(false);
  const sessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (initialized.current) return;

    const initAnalytics = async () => {
      const store = await load('analytics.json', { autoSave: false, defaults: { analyticsOptedIn: true } });
      if (!(await store.has('analyticsOptedIn'))) {
        await store.set('analyticsOptedIn', true);
      }
      const analyticsOptedIn = await store.get('analyticsOptedIn');
      setIsAnalyticsOptedIn(analyticsOptedIn as boolean);
      if (analyticsOptedIn) {
        await initAnalytics2();
      }
    };

    const initAnalytics2 = async () => {
      initialized.current = true;

      const userId = await Analytics.getPersistentUserId();
      await Analytics.init();

      const deviceInfo = await Analytics.getDeviceInfo();
      const appVersion = await Analytics.getAppVersion();

      const store = await load('analytics.json', { autoSave: false, defaults: { analyticsOptedIn: true } });
      await store.set('platform', deviceInfo.platform);
      await store.set('os_version', deviceInfo.os_version);
      await store.set('architecture', deviceInfo.architecture);
      if (!(await store.has('first_launch_date'))) {
        await store.set('first_launch_date', new Date().toISOString());
      }
      await store.save();

      await Analytics.identify(userId, {
        app_version: appVersion,
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version,
        architecture: deviceInfo.architecture,
        first_seen: new Date().toISOString(),
        user_agent: navigator.userAgent,
      });

      const sessionId = await Analytics.startSession(userId);
      if (sessionId) {
        sessionIdRef.current = sessionId;
        await Analytics.trackSessionStarted(sessionId);
      }

      await Analytics.checkAndTrackFirstLaunch();
      await Analytics.trackAppStarted();
      await Analytics.checkAndTrackDailyUsage();

      // Auto-capture unhandled JS errors and promise rejections
      const handleError = (event: ErrorEvent) => {
        Analytics.captureException(event.error ?? new Error(event.message), {
          handled: false,
          filename: event.filename,
          lineno: String(event.lineno),
          colno: String(event.colno),
        });
      };
      const handleRejection = (event: PromiseRejectionEvent) => {
        Analytics.captureException(event.reason ?? new Error('Unhandled promise rejection'), {
          handled: false,
        });
      };
      window.addEventListener('error', handleError);
      window.addEventListener('unhandledrejection', handleRejection);

      // Use Tauri's window close event — more reliable than beforeunload in a Tauri app
      const appWindow = getCurrentWindow();
      const unlistenClose = await appWindow.onCloseRequested(async (event) => {
        event.preventDefault();

        // Flush analytics with a hard 1s timeout — never block the user from closing
        const flush = async () => {
          window.removeEventListener('error', handleError);
          window.removeEventListener('unhandledrejection', handleRejection);
          if (sessionIdRef.current) await Analytics.trackSessionEnded(sessionIdRef.current);
          await Analytics.trackAppClosed();
          await Analytics.cleanup();
        };
        await Promise.race([flush(), new Promise(resolve => setTimeout(resolve, 1000))]);

        appWindow.destroy();
      });

      return () => {
        unlistenClose();
        window.removeEventListener('error', handleError);
        window.removeEventListener('unhandledrejection', handleRejection);
        if (sessionIdRef.current) {
          Analytics.trackSessionEnded(sessionIdRef.current);
        }
        Analytics.trackAppClosed();
        Analytics.cleanup();
      };
    };

    initAnalytics().catch(console.error);
  }, []);

  useEffect(() => {
    if (!isAnalyticsOptedIn) {
      initialized.current = false;
    }
  }, [isAnalyticsOptedIn]);

  return (
    <AnalyticsContext.Provider value={{ isAnalyticsOptedIn, setIsAnalyticsOptedIn }}>
      {children}
    </AnalyticsContext.Provider>
  );
}
