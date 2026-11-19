"use client";

import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';

export type ChatGPTPlanStatus = {
  active_model: string | null;
  profiles: { id: string; email: string; active: boolean }[];
};

const usageUrl = 'https://chatgpt.com/settings/usage';

export function ChatGPTPlanSettings() {
  const [status, setStatus] = useState<ChatGPTPlanStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');

  useEffect(() => {
    let mounted = true;
    invoke<ChatGPTPlanStatus>('chatgpt_plan_status')
      .then(value => { if (mounted && value) setStatus(value); })
      .catch(error => { if (mounted) setMessage(String(error)); });
    return () => { mounted = false; };
  }, []);

  async function connect(profileId: string | null) {
    setBusy(true);
    setMessage('Finish signing in in your browser.');
    try {
      setStatus(await invoke<ChatGPTPlanStatus>('chatgpt_plan_connect', { profileId }));
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
      setStatus(await invoke<ChatGPTPlanStatus>('chatgpt_plan_status'));
      setMessage(revoked ? 'Signed out.' : 'Signed out here. Manage the connection in ChatGPT Settings.');
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  if (!status) return message ? <p role="status" className="text-sm">{message}</p> : null;
  return (
    <section className="space-y-2 border-t pt-4" aria-label="ChatGPT plan">
      <p className="text-sm font-medium">ChatGPT plan</p>
      <p className="text-sm text-muted-foreground">
        Use an eligible ChatGPT plan for summaries. Choose Generate with ChatGPT in a meeting to send its transcript to OpenAI.
      </p>
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
