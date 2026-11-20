"use client";

import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';

export type ChatGPTPlanStatus = {
  summary_default: boolean;
  auto_summary_allowed: boolean;
  active_model: string | null;
  models: { slug: string; display_name: string }[];
  profiles: { id: string; email: string; active: boolean }[];
};

const usageUrl = 'https://chatgpt.com/settings/usage';

export function ChatGPTPlanSettings({ status, onStatusChange, onModelChange }: {
  status: ChatGPTPlanStatus;
  onStatusChange: (status: ChatGPTPlanStatus) => void;
  onModelChange: (model: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');

  const refreshModels = useCallback(async () => {
    setBusy(true);
    setMessage('');
    try {
      onStatusChange(await invoke<ChatGPTPlanStatus>('chatgpt_plan_status', { refreshModels: true }));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }, [onStatusChange]);

  const needsModelList = !!status.active_model && !status.models.length;
  useEffect(() => {
    if (needsModelList) void refreshModels();
  }, [needsModelList, refreshModels]);

  async function connect(profileId: string | null) {
    setBusy(true);
    setMessage('Finish signing in in your browser.');
    try {
      onStatusChange(await invoke<ChatGPTPlanStatus>('chatgpt_plan_connect', { profileId }));
      setMessage('Connected.');
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    setBusy(true);
    try {
      const revoked = await invoke<boolean>('chatgpt_plan_disconnect');
      onStatusChange(await invoke<ChatGPTPlanStatus>('chatgpt_plan_status'));
      setMessage(revoked ? 'Signed out.' : 'Signed out here. Manage the connection in ChatGPT Settings.');
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="space-y-2 border-t pt-4" aria-label="ChatGPT plan">
      <p className="text-sm font-medium">ChatGPT plan</p>
      <p className="text-sm text-muted-foreground">
        Use an eligible ChatGPT plan for summaries. Meetily asks before sending a transcript for a manual summary. Turn on Auto Summary to send transcripts to OpenAI after recording.
      </p>
      {status.active_model && (
        <div className="space-y-1">
          {status.models.length > 0 && (
            <Select value={status.active_model} onValueChange={onModelChange} disabled={busy}>
              <SelectTrigger aria-label="ChatGPT summary model">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {status.models.map(model => (
                  <SelectItem key={model.slug} value={model.slug}>{model.display_name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-muted-foreground">Luna uses Medium reasoning. Terra uses Low.</p>
            <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void refreshModels()}>
              Refresh models
            </Button>
          </div>
        </div>
      )}
      {status.profiles.map((profile, index) => (
        <div key={profile.id} className="flex flex-wrap items-center gap-2 text-sm">
          <span>{profile.email} · Connection {index + 1}{profile.active ? ' · Active' : ''}</span>
          <Button type="button" size="sm" variant="outline" disabled={busy} onClick={() => void connect(profile.id)}>
            {profile.active ? 'Reconnect' : 'Continue with this account'}
          </Button>
          {profile.active && <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => void disconnect()}>Sign out</Button>}
        </div>
      ))}
      <div className="flex flex-wrap items-center gap-2">
        <Button type="button" size="sm" variant="outline" disabled={busy}
          data-oai-siwc-button onClick={() => void connect(null)}>
          {status.profiles.length ? 'Add ChatGPT account' : 'Continue with ChatGPT'}
        </Button>
        <Button type="button" variant="link" size="sm" className="px-0"
          onClick={() => void invoke('open_external_url', { url: usageUrl }).catch(error => setMessage(String(error)))}>
          Manage usage
        </Button>
      </div>
      {message && <p role="status" className="text-sm text-muted-foreground">{message}</p>}
    </section>
  );
}
