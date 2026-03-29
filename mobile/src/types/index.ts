// Shared types — aligned with desktop frontend types and cloud API models

export interface Block {
  id: string
  type: string
  content: string
  color: string
}

export interface Section {
  title: string
  blocks: Block[]
}

export interface Summary {
  [key: string]: Section
}

export interface SummaryDataResponse {
  MeetingName?: string
  _section_order?: string[]
  [key: string]: any
}

export interface TranscriptSegment {
  text: string
  start: number
  end: number
  confidence: number
}

// Cloud meeting — stored in local SQLite and synced to cloud MongoDB
export interface Meeting {
  meeting_id: string
  title: string
  created_at: string
  updated_at: string
  status: MeetingStatus
  duration_seconds?: number
  transcript_text?: string
  transcript_segments?: TranscriptSegment[]
  summary?: SummaryDataResponse
  audio_file_path?: string
  sync_status: SyncStatus
  version: number
  last_synced_at?: string
}

export type MeetingStatus =
  | 'recording'
  | 'pending_upload'
  | 'uploading'
  | 'transcribing'
  | 'summarizing'
  | 'completed'
  | 'error'

export type SyncStatus =
  | 'local_only'
  | 'synced'
  | 'pending_sync'
  | 'conflict'

// Sync queue entry
export interface SyncQueueEntry {
  id: number
  operation: SyncOperation
  meeting_id: string
  payload: string // JSON
  created_at: string
  retry_count: number
  status: 'pending' | 'in_progress' | 'failed'
}

export type SyncOperation =
  | 'create'
  | 'update'
  | 'delete'
  | 'upload_audio'
  | 'request_transcription'
  | 'request_summary'

// Auth types (shared with desktop)
export interface UserProfile {
  user_id: string
  email: string
  display_name: string | null
  devices: DeviceSummary[]
  account_level?: string
  email_verified?: boolean
  org_id?: string | null
  org_role?: string | null
  org_name?: string | null
  org_brand_template_id?: string | null
}

export interface DeviceSummary {
  device_id: string
  platform: string
  linked_at: string
  last_seen: string
}

export interface AuthResponse {
  access_token: string
  refresh_token: string
  user: UserProfile
}

// Usage tracking
export interface UsageEvent {
  event_type: string
  value: number
  metadata?: Record<string, any>
  timestamp: string
}

// Transcription status from cloud API
export interface TranscriptionStatus {
  id: string
  status: 'pending' | 'processing' | 'completed' | 'failed'
  progress?: number
  transcript?: {
    text: string
    segments: TranscriptSegment[]
    duration_seconds: number
    language: string
  }
  error?: string
}

// Summary status from cloud API
export interface SummaryStatus {
  status: 'pending' | 'processing' | 'completed' | 'failed'
  meeting_id: string
  data?: SummaryDataResponse
  error?: string
}
