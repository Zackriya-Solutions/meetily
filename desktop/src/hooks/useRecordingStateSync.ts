import { useEffect, useState } from 'react';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';

interface UseRecordingStateSyncReturn {
  isBackendRecording: boolean;
  isRecordingDisabled: boolean;
  setIsRecordingDisabled: (value: boolean) => void;
}

export function useRecordingStateSync(
  isRecording: boolean,
  setIsRecording: (value: boolean) => void,
  setIsMeetingActive: (value: boolean) => void
): UseRecordingStateSyncReturn {
  const recordingState = useRecordingState();
  const [isRecordingDisabled, setIsRecordingDisabled] = useState(false);

  useEffect(() => {
    if (recordingState.isRecording !== isRecording) {
      setIsRecording(recordingState.isRecording);
    }

    setIsMeetingActive(recordingState.isRecording);
  }, [
    isRecording,
    recordingState.isRecording,
    setIsMeetingActive,
    setIsRecording,
  ]);

  useEffect(() => {
    if (
      recordingState.status === RecordingStatus.IDLE ||
      recordingState.status === RecordingStatus.ERROR ||
      recordingState.status === RecordingStatus.COMPLETED
    ) {
      setIsRecordingDisabled(false);
    }
  }, [recordingState.status]);

  return {
    isBackendRecording: recordingState.isRecording,
    isRecordingDisabled,
    setIsRecordingDisabled,
  };
}
