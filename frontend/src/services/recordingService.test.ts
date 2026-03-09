import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { RecordingService, recordingService } from './recordingService';

describe('RecordingService', () => {
  let service: RecordingService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new RecordingService();
  });

  describe('isRecording', () => {
    it('should return true when recording', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(true);

      const result = await service.isRecording();
      expect(result).toBe(true);
      expect(invoke).toHaveBeenCalledWith('is_recording');
    });

    it('should return false when not recording', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(false);

      const result = await service.isRecording();
      expect(result).toBe(false);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'));
      await expect(service.isRecording()).rejects.toThrow('fail');
    });
  });

  describe('getRecordingState', () => {
    it('should return full recording state when recording', async () => {
      const state = {
        is_recording: true,
        is_paused: false,
        is_active: true,
        recording_duration: 120.5,
        active_duration: 100.3,
      };
      vi.mocked(invoke).mockResolvedValueOnce(state);

      const result = await service.getRecordingState();
      expect(result).toEqual(state);
      expect(invoke).toHaveBeenCalledWith('get_recording_state');
    });

    it('should return idle state when not recording', async () => {
      const state = {
        is_recording: false,
        is_paused: false,
        is_active: false,
        recording_duration: null,
        active_duration: null,
      };
      vi.mocked(invoke).mockResolvedValueOnce(state);

      const result = await service.getRecordingState();
      expect(result.recording_duration).toBeNull();
      expect(result.active_duration).toBeNull();
    });

    it('should return paused state', async () => {
      const state = {
        is_recording: true,
        is_paused: true,
        is_active: false,
        recording_duration: 60.0,
        active_duration: 45.0,
      };
      vi.mocked(invoke).mockResolvedValueOnce(state);

      const result = await service.getRecordingState();
      expect(result.is_paused).toBe(true);
      expect(result.is_active).toBe(false);
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('state error'));
      await expect(service.getRecordingState()).rejects.toThrow('state error');
    });
  });

  describe('getRecordingMeetingName', () => {
    it('should return meeting name', async () => {
      vi.mocked(invoke).mockResolvedValueOnce('Team Standup');

      const result = await service.getRecordingMeetingName();
      expect(result).toBe('Team Standup');
      expect(invoke).toHaveBeenCalledWith('get_recording_meeting_name');
    });

    it('should return null when no meeting name', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(null);

      const result = await service.getRecordingMeetingName();
      expect(result).toBeNull();
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'));
      await expect(service.getRecordingMeetingName()).rejects.toThrow('fail');
    });
  });

  describe('startRecording', () => {
    it('should invoke start_recording', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.startRecording();
      expect(invoke).toHaveBeenCalledWith('start_recording');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('no mic'));
      await expect(service.startRecording()).rejects.toThrow('no mic');
    });
  });

  describe('startRecordingWithDevices', () => {
    it('should invoke start_recording_with_devices_and_meeting with all params', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.startRecordingWithDevices('Mic A', 'Speakers B', 'Daily Standup');
      expect(invoke).toHaveBeenCalledWith('start_recording_with_devices_and_meeting', {
        mic_device_name: 'Mic A',
        system_device_name: 'Speakers B',
        meeting_name: 'Daily Standup',
      });
    });

    it('should handle null device names', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.startRecordingWithDevices(null, null, 'Meeting');
      expect(invoke).toHaveBeenCalledWith('start_recording_with_devices_and_meeting', {
        mic_device_name: null,
        system_device_name: null,
        meeting_name: 'Meeting',
      });
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('device not found'));
      await expect(service.startRecordingWithDevices('Bad Mic', null, 'Test'))
        .rejects.toThrow('device not found');
    });
  });

  describe('stopRecording', () => {
    it('should invoke stop_recording with save path', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.stopRecording('/recordings/meeting.wav');
      expect(invoke).toHaveBeenCalledWith('stop_recording', {
        args: { save_path: '/recordings/meeting.wav' },
      });
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('write error'));
      await expect(service.stopRecording('/bad/path')).rejects.toThrow('write error');
    });
  });

  describe('pauseRecording', () => {
    it('should invoke pause_recording', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.pauseRecording();
      expect(invoke).toHaveBeenCalledWith('pause_recording');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('not recording'));
      await expect(service.pauseRecording()).rejects.toThrow('not recording');
    });
  });

  describe('resumeRecording', () => {
    it('should invoke resume_recording', async () => {
      vi.mocked(invoke).mockResolvedValueOnce(undefined);

      await service.resumeRecording();
      expect(invoke).toHaveBeenCalledWith('resume_recording');
    });

    it('should propagate invoke errors', async () => {
      vi.mocked(invoke).mockRejectedValueOnce(new Error('not paused'));
      await expect(service.resumeRecording()).rejects.toThrow('not paused');
    });
  });

  // Event listeners

  describe('onRecordingStarted', () => {
    it('should listen for recording-started events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onRecordingStarted(callback);

      expect(listen).toHaveBeenCalledWith('recording-started', callback);
      expect(result).toBe(unlisten);
    });
  });

  describe('onRecordingStopped', () => {
    it('should listen for recording-stopped events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onRecordingStopped(callback);

      expect(listen).toHaveBeenCalledWith('recording-stopped', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should pass payload to callback', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onRecordingStopped(callback);

      const payload = { message: 'Recording saved', folder_path: '/tmp/rec', meeting_name: 'Test' };
      capturedHandler({ payload });
      expect(callback).toHaveBeenCalledWith(payload);
    });
  });

  describe('onRecordingPaused', () => {
    it('should listen for recording-paused events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onRecordingPaused(callback);

      expect(listen).toHaveBeenCalledWith('recording-paused', callback);
      expect(result).toBe(unlisten);
    });
  });

  describe('onRecordingResumed', () => {
    it('should listen for recording-resumed events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onRecordingResumed(callback);

      expect(listen).toHaveBeenCalledWith('recording-resumed', callback);
      expect(result).toBe(unlisten);
    });
  });

  describe('onChunkDropWarning', () => {
    it('should listen for chunk-drop-warning events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onChunkDropWarning(callback);

      expect(listen).toHaveBeenCalledWith('chunk-drop-warning', expect.any(Function));
      expect(result).toBe(unlisten);
    });

    it('should pass warning string to callback', async () => {
      let capturedHandler: any;
      vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
        capturedHandler = handler;
        return vi.fn();
      });

      const callback = vi.fn();
      await service.onChunkDropWarning(callback);

      capturedHandler({ payload: 'Buffer overflow: 5 chunks dropped' });
      expect(callback).toHaveBeenCalledWith('Buffer overflow: 5 chunks dropped');
    });
  });

  describe('onSpeechDetected', () => {
    it('should listen for speech-detected events', async () => {
      const unlisten = vi.fn();
      vi.mocked(listen).mockResolvedValueOnce(unlisten);

      const callback = vi.fn();
      const result = await service.onSpeechDetected(callback);

      expect(listen).toHaveBeenCalledWith('speech-detected', callback);
      expect(result).toBe(unlisten);
    });
  });

  describe('singleton export', () => {
    it('should export a singleton instance', () => {
      expect(recordingService).toBeInstanceOf(RecordingService);
    });
  });
});
