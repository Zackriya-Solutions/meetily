'use client';

import React, { useEffect, ReactNode, useRef, useState, createContext } from 'react';
import { useSession } from 'next-auth/react';
import Analytics from '@/lib/analytics';

interface AnalyticsProviderProps {
  children: ReactNode;
}

interface AnalyticsContextType {
  isAnalyticsOptedIn: boolean;
  setIsAnalyticsOptedIn: (optedIn: boolean) => void;
}

export const AnalyticsContext = createContext<AnalyticsContextType>({
  isAnalyticsOptedIn: true,
  setIsAnalyticsOptedIn: () => { },
});

export default function AnalyticsProvider({ children }: AnalyticsProviderProps) {
  const { data: session, status } = useSession();
  const [isAnalyticsOptedIn, setIsAnalyticsOptedIn] = useState(true);
  const initialized = useRef(false);

  // Automatically update the Analytics internal user ID when the session loads
  useEffect(() => {
    if (status === 'authenticated' && session?.user?.email) {
      Analytics.identify(session.user.email, {
        name: session.user.name || undefined
      }).catch(console.error);
    }
  }, [status, session]);

  useEffect(() => {
    // Prevent duplicate initialization in React StrictMode unless the session email just loaded
    if (initialized.current && session?.user?.email === Analytics.getCurrentUserId()) {
      return;
    }
    // Update if the email just became available
    if (initialized.current && session?.user?.email) {
      Analytics.identify(session.user.email);
      return;
    }

    const initAnalytics = async () => {
      // Load preference from localStorage
      const storedOptIn = localStorage.getItem('analyticsOptedIn');
      // Default to true if not set
      let analyticsOptedIn = true;
      if (storedOptIn !== null) {
        analyticsOptedIn = storedOptIn === 'true';
      } else {
        localStorage.setItem('analyticsOptedIn', 'true');
      }

      setIsAnalyticsOptedIn(analyticsOptedIn);

      if (analyticsOptedIn) {
        initAnalytics2();
      }
    }

    const initAnalytics2 = async () => {
      // Mark as initialized to prevent duplicates
      initialized.current = true;

      // Get persistent user ID or use session email if available
      const userId = (session?.user?.email) || await Analytics.getPersistentUserId();

      // Initialize analytics
      await Analytics.init(userId);

      // Get device info
      const deviceInfo = await Analytics.getDeviceInfo();

      // Store platform info if needed (skipping implementation details for local store)

      // Identify user
      await Analytics.identify(userId, {
        app_version: '0.1.1',
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version,
        architecture: deviceInfo.architecture,
        first_seen: new Date().toISOString(),
        user_agent: navigator.userAgent,
      });

      // Start analytics session
      const sessionId = await Analytics.startSession(userId);
      if (sessionId) {
        await Analytics.trackSessionStarted(sessionId);
      }

      // Check and track first launch
      await Analytics.checkAndTrackFirstLaunch();

      // Track app started
      await Analytics.trackAppStarted();

      // Check and track daily usage
      await Analytics.checkAndTrackDailyUsage();

      // Set up cleanup on page unload
      const handleBeforeUnload = async () => {
        if (sessionId) {
          await Analytics.trackSessionEnded(sessionId);
        }
        await Analytics.cleanup();
      };

      window.addEventListener('beforeunload', handleBeforeUnload);

      // Cleanup function
      return () => {
        window.removeEventListener('beforeunload', handleBeforeUnload);
        if (sessionId) {
          Analytics.trackSessionEnded(sessionId);
        }
        Analytics.cleanup();
      };
    };

    initAnalytics().catch(console.error);
  }, [session?.user?.email]); // Re-run if email loads but keep inner initialized.current check so we don't spam

  // Separate effect to handle re-initialization when analytics is toggled
  useEffect(() => {
    // Reset initialized flag when analytics is disabled to allow re-initialization
    if (!isAnalyticsOptedIn) {
      initialized.current = false;
    }
  }, [isAnalyticsOptedIn]);

  return <AnalyticsContext.Provider value={{ isAnalyticsOptedIn, setIsAnalyticsOptedIn }}>{children}</AnalyticsContext.Provider>;
}
