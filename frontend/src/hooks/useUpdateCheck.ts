import { useEffect, useState, useCallback, useRef } from 'react';
import { updateService, UpdateInfo } from '@/services/updateService';
import { showUpdateNotification } from '@/components/UpdateNotification';
import Analytics from '@/lib/analytics';

interface UseUpdateCheckOptions {
  checkOnMount?: boolean;
  showNotification?: boolean;
  onUpdateAvailable?: (info: UpdateInfo) => void;
}

export function useUpdateCheck(options: UseUpdateCheckOptions = {}) {
  const {
    checkOnMount = true,
    showNotification = true,
    onUpdateAvailable,
  } = options;

  // Keep a ref to the latest callback so checkForUpdates doesn't need it as a dep
  const onUpdateAvailableRef = useRef(onUpdateAvailable);
  useEffect(() => { onUpdateAvailableRef.current = onUpdateAvailable; }, [onUpdateAvailable]);

  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isChecking, setIsChecking] = useState(false);

  const checkForUpdates = useCallback(async (force = false) => {
    // Skip if checked recently (unless forced)
    if (!force && updateService.wasCheckedRecently()) {
      return;
    }

    setIsChecking(true);
    try {
      const info = await updateService.checkForUpdates(force);
      setUpdateInfo(info);

      if (info.available) {
        Analytics.trackUpdateAvailable(info.version || 'unknown', info.currentVersion).catch(console.error);
        if (onUpdateAvailableRef.current) {
          onUpdateAvailableRef.current(info);
        } else if (showNotification) {
          showUpdateNotification(info, () => {});
        }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (!message.includes('Could not fetch') && !message.includes('release JSON')) {
        console.error('Failed to check for updates:', error);
      }
    } finally {
      setIsChecking(false);
    }
  }, [showNotification]); // stable — callbacks accessed via ref

  useEffect(() => {
    if (checkOnMount) {
      // Delay the check slightly to avoid blocking app startup
      const timer = setTimeout(() => {
        checkForUpdates(false);
      }, 2000); // Check 2 seconds after mount

      return () => clearTimeout(timer);
    }
  }, [checkOnMount]);

  return {
    updateInfo,
    isChecking,
    checkForUpdates,
  };
}
