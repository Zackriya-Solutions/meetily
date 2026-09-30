'use client';

import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select';
import { Input } from './ui/input';
import { Textarea } from './ui/textarea';
import { Button } from './ui/button';
import { Label } from './ui/label';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './ui/dialog';
import { FolderOpen, Pencil, RefreshCw, Trash2 } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';

const NONE_VALUE = '__none__';

type Mode = 'list' | 'edit';

/** Mirrors `meeting_domain::tokens::PromptTokenCount` in Rust. */
interface PromptTokenCount {
  tokens: number;
  limit: number;
  model: string;
  dropped_text: string | null;
}

export function MeetingDomainSettings() {
  const { selectedDomain, setSelectedDomain, transcriptModelConfig } = useConfig();
  const [domains, setDomains] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<Mode>('list');
  const [editingName, setEditingName] = useState('');
  const [editingOriginal, setEditingOriginal] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [tokenCount, setTokenCount] = useState<PromptTokenCount | null>(null);
  const [tokenCountError, setTokenCountError] = useState(false);

  // Count the prompt in Whisper tokens while editing. The model's vocabulary
  // is read from its file header, so this works without a loaded model.
  const whisperModel =
    transcriptModelConfig.provider === 'localWhisper' ? transcriptModelConfig.model : null;
  useEffect(() => {
    if (mode !== 'edit') return;
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const count = await invoke<PromptTokenCount>('count_meeting_domain_tokens', {
          content: editingContent,
          model: whisperModel,
        });
        if (!cancelled) {
          setTokenCount(count);
          setTokenCountError(false);
        }
      } catch (err) {
        console.error('Failed to count meeting domain tokens:', err);
        if (!cancelled) {
          setTokenCount(null);
          setTokenCountError(true);
        }
      }
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [mode, editingContent, whisperModel]);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<string[]>('list_meeting_domains');
      setDomains(list);
    } catch (err) {
      console.error('Failed to list meeting domains:', err);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleSelect = (value: string) => {
    setSelectedDomain(value === NONE_VALUE ? '' : value);
  };

  const startCreate = () => {
    setMode('edit');
    setEditingOriginal(null);
    setEditingName('');
    setEditingContent('');
    setTokenCount(null);
    setError(null);
  };

  const startEdit = async (name: string) => {
    setMode('edit');
    setEditingOriginal(name);
    setEditingName(name);
    setTokenCount(null);
    setError(null);
    try {
      const content = await invoke<string | null>('get_meeting_domain_content', { name });
      setEditingContent(content ?? '');
    } catch (err) {
      setError(String(err));
      setEditingContent('');
    }
  };

  const handleDelete = async (name: string) => {
    if (!confirm(`Delete meeting domain "${name}"?`)) return;
    try {
      await invoke('delete_meeting_domain', { name });
      if (selectedDomain === name) {
        setSelectedDomain('');
      }
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const targetName = editingName.trim();
      if (!targetName) {
        setError('Name is required');
        setSaving(false);
        return;
      }
      await invoke('save_meeting_domain', {
        name: targetName,
        content: editingContent,
      });
      // If renaming an existing domain, remove the old file.
      if (editingOriginal && editingOriginal !== targetName) {
        await invoke('delete_meeting_domain', { name: editingOriginal });
        if (selectedDomain === editingOriginal) {
          setSelectedDomain(targetName);
        }
      }
      await refresh();
      setMode('list');
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_meeting_domains_folder');
    } catch (err) {
      console.error('Failed to open meeting domains folder:', err);
    }
  };

  return (
    <div>
      <Label className="block text-sm font-medium text-gray-700 mb-1">
        Meeting Domain
      </Label>
      <p className="text-xs text-gray-500 mb-2 mx-1">
        Biases Whisper transcription toward a custom vocabulary file
        (e.g. proper nouns, client names). Leave as "None" for default behavior.
      </p>
      <div className="flex space-x-2 mx-1">
        <Select
          value={selectedDomain ? selectedDomain : NONE_VALUE}
          onValueChange={handleSelect}
        >
          <SelectTrigger className="focus:ring-1 focus:ring-blue-500 focus:border-blue-500">
            <SelectValue placeholder="None" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NONE_VALUE}>None (default)</SelectItem>
            {domains.map((d) => (
              <SelectItem key={d} value={d}>
                {d}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="outline"
          onClick={refresh}
          title="Rescan domain folder"
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => {
            setMode('list');
            setError(null);
            setOpen(true);
          }}
        >
          Manage…
        </Button>
      </div>

      <Dialog
        open={open}
        onOpenChange={(o) => {
          setOpen(o);
          if (!o) {
            setMode('list');
            setError(null);
          }
        }}
      >
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>Manage Meeting Domains</DialogTitle>
            <DialogDescription>
              Each domain is a plain-text vocabulary hint sent to Whisper as
              <code className="mx-1">initial_prompt</code>. Whisper uses at most
              the last 224 tokens (Norwegian names often cost 2–3 tokens each),
              so put the most important terms last.
            </DialogDescription>
          </DialogHeader>

          {error && (
            <div className="text-sm text-red-600 bg-red-50 border border-red-200 rounded p-2">
              {error}
            </div>
          )}

          {mode === 'list' ? (
            <div className="space-y-3">
              <div className="max-h-72 overflow-y-auto border rounded">
                {domains.length === 0 ? (
                  <div className="p-4 text-sm text-gray-500 text-center">
                    No domains yet. Click "+ Add new" to create one.
                  </div>
                ) : (
                  <ul>
                    {domains.map((d) => (
                      <li
                        key={d}
                        className="flex items-center justify-between p-2 border-b last:border-b-0 hover:bg-gray-50"
                      >
                        <span className="text-sm font-mono">{d}</span>
                        <div className="flex space-x-1">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => startEdit(d)}
                            title="Edit"
                          >
                            <Pencil className="h-4 w-4" />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => handleDelete(d)}
                            title="Delete"
                          >
                            <Trash2 className="h-4 w-4 text-red-500" />
                          </Button>
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              <div className="flex justify-between">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleOpenFolder}
                >
                  <FolderOpen className="h-4 w-4 mr-1" />
                  Open folder
                </Button>
                <Button type="button" size="sm" onClick={startCreate}>
                  + Add new
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <div>
                <Label className="text-sm font-medium">Name</Label>
                <Input
                  value={editingName}
                  onChange={(e) => setEditingName(e.target.value)}
                  placeholder="e.g. tekni"
                  className="mt-1"
                />
                <p className="text-xs text-gray-500 mt-1">
                  Lowercase letters, digits, dashes, underscores. No spaces or slashes.
                </p>
              </div>
              <div>
                <Label className="text-sm font-medium">Prompt content</Label>
                <Textarea
                  value={editingContent}
                  onChange={(e) => setEditingContent(e.target.value)}
                  placeholder="Names, terms, acronyms separated by commas or periods…"
                  rows={10}
                  className="mt-1 font-mono text-sm"
                />
                <p className="text-xs text-gray-500 mt-1">
                  {tokenCount ? (
                    <>
                      <span
                        className={
                          tokenCount.tokens > tokenCount.limit
                            ? 'font-medium text-red-600'
                            : 'font-medium text-gray-700'
                        }
                      >
                        {tokenCount.tokens} / {tokenCount.limit} tokens
                      </span>
                      {' · '}
                      {editingContent.trim().length} chars
                    </>
                  ) : (
                    <>
                      {editingContent.trim().length} chars
                      {tokenCountError && ' — token count unavailable (no Whisper model found)'}
                    </>
                  )}
                </p>
                {tokenCount && tokenCount.tokens > tokenCount.limit && (
                  <div className="text-xs text-red-700 bg-red-50 border border-red-200 rounded p-2 mt-2">
                    Over the limit by {tokenCount.tokens - tokenCount.limit} tokens. Whisper
                    only uses the last {tokenCount.limit}, so this start has no effect:
                    {tokenCount.dropped_text && (
                      <span className="block mt-1 font-mono break-words">
                        “{tokenCount.dropped_text}”
                      </span>
                    )}
                    <span className="block mt-1">
                      Shorten the list or move the most important terms to the end.
                    </span>
                  </div>
                )}
              </div>
              <DialogFooter>
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setMode('list')}
                  disabled={saving}
                >
                  Cancel
                </Button>
                <Button type="button" onClick={handleSave} disabled={saving}>
                  {saving ? 'Saving…' : 'Save'}
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
