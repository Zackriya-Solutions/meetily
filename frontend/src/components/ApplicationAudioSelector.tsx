import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Switch } from '@/components/ui/switch';
import { applicationKey, selectedApplicationAvailable, type AudioApplication, type CaptureCapabilities, type RecordingSourceOptions } from '@/lib/recording-source';

interface Props {
  options: RecordingSourceOptions;
  onChange: (options: RecordingSourceOptions) => void;
  disabled: boolean;
}

export function ApplicationAudioSelector({ options, onChange, disabled }: Props) {
  const [capabilities, setCapabilities] = useState<CaptureCapabilities | null>(null);
  const [apps, setApps] = useState<AudioApplication[]>([]);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const appMode = options.source.kind === 'application';
  const selected = options.source.kind === 'application' ? options.source.application : null;

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try { setApps(await invoke<AudioApplication[]>('list_audio_applications')); }
    catch (error) { setError(String(error)); }
    finally { setLoading(false); }
  };
  useEffect(() => {
    let cancelled = false;
    invoke<CaptureCapabilities>('get_audio_capture_capabilities').then(value => {
      if (!cancelled) setCapabilities(value);
    }).catch(error => { if (!cancelled) setError(String(error)); });
    return () => { cancelled = true; };
  }, []);
  useEffect(() => {
    if (capabilities?.applicationAudio && appMode && !disabled) void refresh();
  }, [capabilities?.applicationAudio, appMode, disabled]);

  const visibleApps = apps.filter(app => app.name.toLowerCase().includes(query.toLowerCase()));
  // Keep the saved selection readable even when unavailable or filtered out.
  const includeSaved = selected?.bundleId && !visibleApps.some(app => applicationKey(app) === applicationKey(selected));
  const unavailable = !loading && !error && !selectedApplicationAvailable(apps, options.source);

  return <div className="space-y-3">
    <div className="flex items-center justify-between gap-3">
      <label htmlFor="include-microphone" className="text-sm font-medium">Include microphone</label>
      <Switch id="include-microphone" checked={options.microphoneEnabled} disabled={disabled}
        onCheckedChange={microphoneEnabled => onChange({ ...options, microphoneEnabled })} />
    </div>
    <div className="space-y-1">
      <label htmlFor="audio-source-mode" className="text-sm font-medium">Audio source</label>
      <select id="audio-source-mode" value={options.source.kind} disabled={disabled || !capabilities}
        className="w-full rounded-md border bg-white p-2 text-sm disabled:opacity-50"
        onChange={event => onChange({ ...options, source: event.target.value === 'system'
          ? { kind: 'system' }
          : { kind: 'application', application: { bundleId: '', bundlePath: '', name: '' } } })}>
        <option value="system">All system audio</option>
        <option value="application" disabled={!capabilities?.applicationAudio}>Specific application</option>
      </select>
      {capabilities && !capabilities.applicationAudio && <p className="text-xs text-gray-500">{capabilities.reason}</p>}
    </div>
    {appMode && <div className="space-y-2">
      <label htmlFor="audio-application-search" className="text-sm font-medium">Application</label>
      <div className="flex gap-2">
        <input id="audio-application-search" type="search" value={query} placeholder="Search running apps"
          disabled={disabled} onChange={event => setQuery(event.target.value)}
          className="min-w-0 flex-1 rounded-md border p-2 text-sm" />
        <button type="button" disabled={disabled || loading || !capabilities?.applicationAudio} onClick={() => void refresh()}
          className="rounded-md border [padding-inline:0.75rem] text-sm disabled:opacity-50 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 active:opacity-80">{loading ? 'Loading…' : 'Refresh'}</button>
      </div>
      <select aria-label="Application to record" value={selected?.bundleId ? applicationKey(selected) : ''}
        disabled={disabled || loading || !capabilities?.applicationAudio}
        className="w-full rounded-md border bg-white p-2 text-sm disabled:opacity-50"
        onChange={event => {
          const app = apps.find(app => applicationKey(app) === event.target.value);
          if (app) onChange({ ...options, source: { kind: 'application', application: {
            bundleId: app.bundleId, bundlePath: app.bundlePath, name: app.name,
          } } });
        }}>
        <option value="" disabled>Select a running application</option>
        {includeSaved && selected && <option value={applicationKey(selected)}>{selected.name}{unavailable ? ' — unavailable' : ''}</option>}
        {visibleApps.map(app => <option key={applicationKey(app)} value={applicationKey(app)}>
          {app.name} — {app.audioActive ? 'Audio active' : 'Idle'}
        </option>)}
      </select>
      {unavailable && selected?.bundleId && <p role="status" className="text-sm text-amber-700">Open {selected.name} and refresh before recording.</p>}
      {!loading && !error && visibleApps.length === 0 && <p className="text-xs text-gray-500">No matching running apps. Open your meeting app and refresh.</p>}
      <p className="text-xs text-gray-500">Browser capture may include audio from other tabs. If the app closes, Meetily waits and reconnects automatically.</p>
    </div>}
    {error && <p role="alert" className="text-sm text-red-700">{error}</p>}
  </div>;
}
