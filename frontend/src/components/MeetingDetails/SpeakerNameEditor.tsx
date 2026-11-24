'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { TranscriptSegmentData } from '@/types';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

interface SpeakerNameEditorProps {
  meetingId: string;
  /** The line whose speaker is being named; null closes the dialog. */
  segment: TranscriptSegmentData | null;
  /** Names already found in this meeting, offered as one-click picks. */
  suggestions: string[];
  onClose: () => void;
  /** The ids of the lines whose name changed, and the new name. */
  onSaved: (transcriptIds: string[], name: string | null) => Promise<void> | void;
}

/**
 * Names the speaker of a line: for every line that voice spoke in this
 * meeting, or for this line alone when the voice was mixed up. The names are
 * kept next to the automatic labels, so the speaker accuracy can be measured.
 */
export function SpeakerNameEditor({ meetingId, segment, suggestions, onClose, onSaved }: SpeakerNameEditorProps) {
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setName(segment?.speakerName ?? '');
  }, [segment]);

  const save = async (scope: 'speaker' | 'line', value: string | null) => {
    if (!segment) return;
    setSaving(true);
    try {
      let changed: string[];
      if (scope === 'speaker' && segment.speakerLabel) {
        changed = await invoke<string[]>('name_meeting_speaker', { meetingId, speakerLabel: segment.speakerLabel, name: value });
      } else {
        await invoke('set_transcript_speaker_name', { transcriptId: segment.id, name: value });
        changed = [segment.id];
      }
      await onSaved(changed, value);
      onClose();
    } catch (error) {
      console.error('Failed to save speaker name:', error);
      toast.error(`Could not save the name: ${String(error)}`);
    } finally {
      setSaving(false);
    }
  };

  const trimmed = name.trim();
  const label = segment?.speakerLabel;

  return (
    <Dialog open={segment !== null} onOpenChange={open => { if (!open) onClose(); }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Who is speaking?</DialogTitle>
          <DialogDescription>
            {label ? `This line is labelled ${label}.` : 'This line has no voice label yet.'}
          </DialogDescription>
        </DialogHeader>

        {segment && (
          <p className="text-sm text-gray-600 line-clamp-3 border-l-2 border-gray-200 pl-3">{segment.text}</p>
        )}

        <label className="flex flex-col gap-1 text-sm font-medium text-gray-700">
          Name
          <input
            autoFocus
            value={name}
            onChange={e => setName(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && trimmed && !saving) void save(label ? 'speaker' : 'line', trimmed);
            }}
            placeholder="e.g. Alex"
            className="rounded-md border border-gray-300 px-3 py-2 text-base font-normal focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </label>

        {suggestions.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {suggestions.map(s => (
              <button
                key={s}
                type="button"
                onClick={() => setName(s)}
                className="rounded-full border border-gray-300 px-3 py-1 text-sm text-gray-700 hover:bg-gray-100"
              >
                {s}
              </button>
            ))}
          </div>
        )}

        <DialogFooter className="flex-col gap-2 sm:flex-row sm:justify-between">
          {segment?.speakerName ? (
            <Button variant="ghost" disabled={saving} onClick={() => void save('line', null)}>
              Clear this line
            </Button>
          ) : <span />}
          <div className="flex flex-col gap-2 sm:flex-row">
            <Button variant="outline" disabled={saving || !trimmed} onClick={() => void save('line', trimmed)}>
              Only this line
            </Button>
            {label && (
              <Button disabled={saving || !trimmed} onClick={() => void save('speaker', trimmed)}>
                {`Every ${label} line`}
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
