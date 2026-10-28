export interface AudioApplicationIdentity {
  bundleId: string;
  bundlePath: string;
  name: string;
}

export type AudioSource = { kind: 'system' } | { kind: 'application'; application: AudioApplicationIdentity };
export interface RecordingSourceOptions {
  source: AudioSource;
  microphoneEnabled: boolean;
}
export interface AudioApplication extends AudioApplicationIdentity { audioActive: boolean }
export interface CaptureCapabilities { applicationAudio: boolean; reason: string | null }
export interface RecordingSourceStatus {
  sessionId: string;
  options: RecordingSourceOptions;
  state: 'capturing' | 'waiting' | 'error';
  message: string | null;
}

export const defaultSourceOptions = (): RecordingSourceOptions => ({ source: { kind: 'system' }, microphoneEnabled: true });
export const normalizeSourceOptions = (options?: Partial<RecordingSourceOptions> | null): RecordingSourceOptions => ({
  source: options?.source ?? { kind: 'system' },
  microphoneEnabled: options?.microphoneEnabled ?? true,
});
export const applicationKey = (app: AudioApplicationIdentity) => JSON.stringify([app.bundleId, app.bundlePath]);
export const sourceLabel = (options?: RecordingSourceOptions) => options?.source.kind === 'application'
  ? options.source.application.name || 'Select an application' : 'All system audio';

export function selectedApplicationAvailable(apps: AudioApplication[], source: AudioSource): boolean {
  return source.kind === 'system' || apps.some(app => applicationKey(app) === applicationKey(source.application));
}
