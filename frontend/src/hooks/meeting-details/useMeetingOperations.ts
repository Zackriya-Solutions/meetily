import { useCallback } from 'react';
import { toast } from 'sonner';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { apiUrl } from '@/lib/config';
import { authFetch } from '@/lib/api';

interface UseMeetingOperationsProps {
  meeting: any;
}

export function useMeetingOperations({
  meeting,
}: UseMeetingOperationsProps) {
  const { serverAddress } = useSidebar();
  const baseUrl = serverAddress || apiUrl;

  // Download recording
  const handleDownloadRecording = useCallback(async () => {
    try {
      const response = await authFetch(`/meetings/${meeting.id}/recording-url`);
      if (!response.ok) {
        throw new Error('No recording available');
      }
      
      const data = await response.json();
      if (data.url) {
        // Trigger download
        const link = document.createElement('a');
        link.href = data.url;
        const ext = data.format || 'wav';
        link.download = data.filename || `recording-${meeting.id}.${ext}`;
        link.rel = 'noopener noreferrer';
        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);
      }
    } catch (error) {
      console.error('Failed to download recording:', error);
      toast.error('Failed to download recording');
    }
  }, [meeting.id]);

  // Delete meeting
  const handleDeleteMeeting = useCallback(async (router: any) => {
    try {
      if (!confirm('Are you sure you want to delete this meeting? This action cannot be undone.')) {
        return;
      }

      const response = await authFetch('/delete-meeting', {
        method: 'POST',
        // headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ meeting_id: meeting.id }),
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.detail || 'Failed to delete meeting');
      }

      toast.success('Meeting deleted successfully');
      // Redirect to home page
      router.push('/');
      
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error('Failed to delete meeting', { 
        description: error instanceof Error ? error.message : 'Unknown error'
      });
    }
  }, [meeting.id, baseUrl]);

  return {
    handleDownloadRecording,
    handleDeleteMeeting,
  };
}
