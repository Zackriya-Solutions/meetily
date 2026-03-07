interface StatusOverlaysProps {
  // Status flags
  isProcessing: boolean;      // Processing transcription after recording stops
  isSaving: boolean;          // Saving transcript to database

  // Layout
  sidebarCollapsed: boolean;  // For responsive margin calculation
}

// Internal reusable component for individual status overlays
interface StatusOverlayProps {
  show: boolean;
  message: string;
  sidebarCollapsed: boolean;
}

function StatusOverlay({ show, message, sidebarCollapsed }: StatusOverlayProps) {
  if (!show) return null;

  const sidebarWidth = sidebarCollapsed ? '4rem' : '16rem';

  return (
    <div
      className="fixed bottom-8 z-50 flex justify-center transition-[left] duration-300"
      style={{ left: sidebarWidth, right: 0 }}
    >
      <div className="flex items-center space-x-2 bg-white dark:bg-card rounded-full shadow-lg dark:shadow-black/40 px-4 py-2">
        <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-gray-900 dark:border-foreground flex-shrink-0" />
        <span className="text-sm text-gray-600 dark:text-muted-foreground">{message}</span>
      </div>
    </div>
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
