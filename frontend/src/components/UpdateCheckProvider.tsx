'use client'

import React, { createContext, useContext, useState, useCallback, useEffect } from 'react';
import { useUpdateCheck } from '@/hooks/useUpdateCheck';
import { UpdateInfo } from '@/services/updateService';
import { UpdateDialog } from './UpdateDialog';
import { setUpdateDialogCallback, showUpdateNotification } from './UpdateNotification';

interface UpdateCheckContextType {
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  checkForUpdates: (force?: boolean) => Promise<void>;
  showUpdateDialog: () => void;
}

const UpdateCheckContext = createContext<UpdateCheckContextType | undefined>(undefined);

export function UpdateCheckProvider({ children }: { children: React.ReactNode }) {
  const [showDialog, setShowDialog] = useState(false);
  // Separate state for what the dialog displays — can be cleared to show loading state
  const [dialogUpdateInfo, setDialogUpdateInfo] = useState<UpdateInfo | null>(null);

  const handleShowDialog = useCallback(() => {
    setShowDialog(true);
  }, []);

  const { updateInfo, isChecking, checkForUpdates } = useUpdateCheck({
    checkOnMount: true,
    showNotification: true,
    onUpdateAvailable: (info) => {
      showUpdateNotification(info, handleShowDialog);
    },
  });

  // Keep dialogUpdateInfo in sync with the hook result when dialog is open
  useEffect(() => {
    if (showDialog) {
      setDialogUpdateInfo(updateInfo);
    }
  }, [updateInfo, showDialog]);

  useEffect(() => {
    setUpdateDialogCallback(handleShowDialog);
    return () => { setUpdateDialogCallback(() => {}); };
  }, [handleShowDialog]);

  // Listen for tray menu events
  useEffect(() => {
    const handleTrayCheck = async () => {
      // Clear first so dialog shows loading state immediately
      setDialogUpdateInfo(null);
      setShowDialog(true);
      await checkForUpdates(true);
    };

    window.addEventListener('check-updates-from-tray', handleTrayCheck);
    return () => window.removeEventListener('check-updates-from-tray', handleTrayCheck);
  }, [checkForUpdates]);

  return (
    <UpdateCheckContext.Provider
      value={{
        updateInfo,
        isChecking,
        checkForUpdates,
        showUpdateDialog: handleShowDialog,
      }}
    >
      {children}
      <UpdateDialog
        open={showDialog}
        onOpenChange={setShowDialog}
        updateInfo={dialogUpdateInfo}
      />
    </UpdateCheckContext.Provider>
  );
}

export function useUpdateCheckContext() {
  const context = useContext(UpdateCheckContext);
  if (context === undefined) {
    throw new Error('useUpdateCheckContext must be used within UpdateCheckProvider');
  }
  return context;
}
