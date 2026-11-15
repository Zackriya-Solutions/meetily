import { invoke } from '@tauri-apps/api/core';

/** Speaker id → custom display name for one meeting. */
export type SpeakerNames = Record<string, string>;

export const SPEAKER_ME = 'me';
export const SPEAKER_OTHERS = 'others';

/** Default label for a stored speaker id ('me', 'others', 'speaker_N'). */
export function defaultSpeakerLabel(speakerId: string): string {
  if (speakerId === SPEAKER_ME) return 'Me';
  if (speakerId === SPEAKER_OTHERS) return 'Others';
  const match = /^speaker_(\d+)$/.exec(speakerId);
  if (match) return `Speaker ${match[1]}`;
  return speakerId;
}

export function speakerDisplayName(speakerId: string, names?: SpeakerNames): string {
  return names?.[speakerId]?.trim() || defaultSpeakerLabel(speakerId);
}

const SPEAKER_COLORS = [
  'bg-sky-100 text-sky-800 border-sky-200',
  'bg-amber-100 text-amber-800 border-amber-200',
  'bg-emerald-100 text-emerald-800 border-emerald-200',
  'bg-rose-100 text-rose-800 border-rose-200',
  'bg-violet-100 text-violet-800 border-violet-200',
  'bg-lime-100 text-lime-800 border-lime-200',
  'bg-orange-100 text-orange-800 border-orange-200',
  'bg-teal-100 text-teal-800 border-teal-200',
];

/** Stable Tailwind color classes per speaker id. */
export function speakerColorClasses(speakerId: string): string {
  if (speakerId === SPEAKER_ME) return 'bg-blue-100 text-blue-800 border-blue-200';
  if (speakerId === SPEAKER_OTHERS) return 'bg-gray-100 text-gray-700 border-gray-200';
  const match = /^speaker_(\d+)$/.exec(speakerId);
  const index = match ? Number(match[1]) - 1 : [...speakerId].reduce((a, c) => a + c.charCodeAt(0), 0);
  return SPEAKER_COLORS[((index % SPEAKER_COLORS.length) + SPEAKER_COLORS.length) % SPEAKER_COLORS.length];
}

/** Speaker ids in order of first appearance. */
export function speakersInOrder(items: { speaker?: string | null }[]): string[] {
  const seen = new Set<string>();
  const ordered: string[] = [];
  for (const item of items) {
    if (item.speaker && !seen.has(item.speaker)) {
      seen.add(item.speaker);
      ordered.push(item.speaker);
    }
  }
  return ordered;
}

/** "Name: " prefix for plain-text exports (copy, AI summary), or '' when unlabelled. */
export function speakerTextPrefix(speakerId: string | null | undefined, names?: SpeakerNames): string {
  return speakerId ? `${speakerDisplayName(speakerId, names)}: ` : '';
}

export async function fetchSpeakerNames(meetingId: string): Promise<SpeakerNames> {
  try {
    const rows = await invoke<{ speaker_id: string; display_name: string }[]>('api_get_meeting_speakers', { meetingId });
    return Object.fromEntries(rows.map((r) => [r.speaker_id, r.display_name]));
  } catch (error) {
    console.warn('Failed to load speaker names:', error);
    return {};
  }
}

export function renameMeetingSpeaker(meetingId: string, speakerId: string, displayName: string): Promise<void> {
  return invoke('api_rename_meeting_speaker', { meetingId, speakerId, displayName });
}

export function mergeMeetingSpeakers(meetingId: string, fromSpeakerId: string, toSpeakerId: string): Promise<number> {
  return invoke('api_merge_meeting_speakers', { meetingId, fromSpeakerId, toSpeakerId });
}

export interface SpeakerIdentificationStatus {
  enabled: boolean;
  model_downloaded: boolean;
  downloading: boolean;
  model_size_bytes: number;
}

export function getSpeakerIdentificationStatus(): Promise<SpeakerIdentificationStatus> {
  return invoke('speaker_get_status');
}

export function setSpeakerIdentificationEnabled(enabled: boolean): Promise<void> {
  return invoke('speaker_set_enabled', { enabled });
}

export function downloadSpeakerModel(): Promise<void> {
  return invoke('speaker_download_model');
}

export function deleteSpeakerModel(): Promise<void> {
  return invoke('speaker_delete_model');
}
