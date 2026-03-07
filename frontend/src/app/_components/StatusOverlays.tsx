'use client';

import { AnimatePresence, motion } from 'framer-motion';

interface StatusOverlayProps {
  show: boolean;
  message: string;
  sidebarCollapsed: boolean;
}

interface StatusOverlaysProps {
  isProcessing: boolean;
  isSaving: boolean;
  sidebarCollapsed: boolean;
}

function StatusOverlay({ show, message, sidebarCollapsed }: StatusOverlayProps) {
  const sidebarWidth = sidebarCollapsed ? '4rem' : '16rem';

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          key={message}
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 8 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
          className="fixed bottom-12 z-50 flex justify-center transition-[left] duration-300"
          style={{ left: sidebarWidth, right: 0 }}
        >
          <div className="flex items-center space-x-2 bg-white dark:bg-card rounded-full shadow-lg dark:shadow-black/40 px-4 py-2 border border-transparent dark:border-border">
            <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-gray-900 dark:border-foreground flex-shrink-0" />
            <span className="text-sm text-gray-600 dark:text-muted-foreground">{message}</span>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

// Main exported component - renders multiple status overlays
export function StatusOverlays({
  isProcessing,
  isSaving,
  sidebarCollapsed
}: StatusOverlaysProps) {
  return (
    <>
      {/* Processing status overlay - shown after recording stops while finalizing transcription */}
      <StatusOverlay
        show={isProcessing}
        message="Finalizing transcription..."
        sidebarCollapsed={sidebarCollapsed}
      />

      {/* Saving status overlay - shown while saving transcript to database */}
      <StatusOverlay
        show={isSaving}
        message="Saving transcript..."
        sidebarCollapsed={sidebarCollapsed}
      />
    </>
  );
}
