'use client';

import { motion } from 'framer-motion';
import { useEffect, useState } from 'react';

// Basic prop based status bar
export interface RecordingStatusBarProps {
  isPaused?: boolean;
  isRecording: boolean;
  activeDuration?: number;
}

export const RecordingStatusBar: React.FC<RecordingStatusBarProps> = ({ isPaused = false, isRecording, activeDuration }) => {
  // Use passed activeDuration or just timer
  const [displaySeconds, setDisplaySeconds] = useState(0);

  // Sync with backend duration when it changes (handles refresh/navigation)
  useEffect(() => {
    if (typeof activeDuration === 'number') {
      // Round to nearest second to avoid decimal issues
      setDisplaySeconds(Math.floor(activeDuration));
    }
  }, [activeDuration]);

  // Live timer that increments every second when recording and not paused
  useEffect(() => {
    // If parent provides a duration, trust it as source of truth
    if (typeof activeDuration === 'number') return;

    // Stop timer if not recording or if paused
    if (!isRecording || isPaused) return;

    const interval = setInterval(() => {
      setDisplaySeconds(prev => prev + 1);
    }, 1000);

    return () => clearInterval(interval);
  }, [isRecording, isPaused, activeDuration]);

  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: -10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.2 }}
      className="flex items-center gap-2 px-3 py-2 bg-gray-50 rounded-lg mb-2"
    >
      <div className={`w-2 h-2 rounded-full ${isPaused ? 'bg-orange-500' : 'bg-red-500 animate-pulse'}`} />
      <span className={`text-sm ${isPaused ? 'text-orange-700' : 'text-gray-700'}`}>
        {isPaused ? 'Paused' : 'Recording'} • {formatDuration(displaySeconds)}
      </span>
    </motion.div>
  );
};
