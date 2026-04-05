import type { Transcript } from '@/types';

export interface MeetingDetails {
  id: string;
  title: string;
  created_at: string;
  updated_at?: string;
  transcripts: Transcript[];
  folder_path?: string;
}
