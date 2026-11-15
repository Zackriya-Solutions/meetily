"use client";

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  SpeakerNames,
  defaultSpeakerLabel,
  mergeMeetingSpeakers,
  renameMeetingSpeaker,
  speakerColorClasses,
  speakerDisplayName,
} from '@/lib/speakers';

interface SpeakersBarProps {
  meetingId: string;
  /** Speaker ids present in the transcript, in order of first appearance */
  speakers: string[];
  speakerNames: SpeakerNames;
  onSpeakerNamesChange: (names: SpeakerNames) => void;
  /** Speaker id whose edit dialog is open (controlled, so transcript chips can open it) */
  editingSpeaker: string | null;
  onEditingSpeakerChange: (speakerId: string | null) => void;
  /** Called after a merge changes stored transcript rows */
  onTranscriptsChanged?: () => Promise<void> | void;
}

export function SpeakersBar({
  meetingId,
  speakers,
  speakerNames,
  onSpeakerNamesChange,
  editingSpeaker,
  onEditingSpeakerChange,
  onTranscriptsChanged,
}: SpeakersBarProps) {
  const [nameDraft, setNameDraft] = useState('');
  const [mergeTarget, setMergeTarget] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (editingSpeaker) {
      setNameDraft(speakerNames[editingSpeaker] ?? '');
      setMergeTarget('');
    }
  }, [editingSpeaker, speakerNames]);

  if (speakers.length === 0) {
    return null;
  }

  const close = () => onEditingSpeakerChange(null);

  const handleSave = async () => {
    if (!editingSpeaker) return;
    setBusy(true);
    try {
      if (mergeTarget) {
        await mergeMeetingSpeakers(meetingId, editingSpeaker, mergeTarget);
        const { [editingSpeaker]: _removed, ...rest } = speakerNames;
        onSpeakerNamesChange(rest);
        toast.success(
          `Merged ${speakerDisplayName(editingSpeaker, speakerNames)} into ${speakerDisplayName(mergeTarget, speakerNames)}`
        );
        await onTranscriptsChanged?.();
      } else {
        const name = nameDraft.trim();
        await renameMeetingSpeaker(meetingId, editingSpeaker, name);
        const next = { ...speakerNames };
        if (name) {
          next[editingSpeaker] = name;
        } else {
          delete next[editingSpeaker];
        }
        onSpeakerNamesChange(next);
      }
      close();
    } catch (error) {
      toast.error(typeof error === 'string' ? error : 'Could not update the speaker.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="flex flex-wrap items-center gap-1.5 px-4 py-2 border-b border-gray-200 bg-gray-50">
        <span className="text-xs text-gray-500 mr-1">Speakers</span>
        {speakers.map((id) => (
          <button
            key={id}
            type="button"
            onClick={() => onEditingSpeakerChange(id)}
            title="Rename or merge speaker"
            className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium hover:brightness-95 focus:outline-none focus:ring-1 focus:ring-blue-500 ${speakerColorClasses(id)}`}
          >
            {speakerDisplayName(id, speakerNames)}
          </button>
        ))}
      </div>

      <Dialog open={editingSpeaker !== null} onOpenChange={(open) => !open && close()}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Edit speaker</DialogTitle>
            <DialogDescription>
              Rename {editingSpeaker ? defaultSpeakerLabel(editingSpeaker) : ''} for this meeting, or merge it into
              another speaker if the same person was split in two.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            <div className="space-y-1">
              <label htmlFor="speaker-name" className="text-sm font-medium text-gray-700">Name</label>
              <Input
                id="speaker-name"
                value={nameDraft}
                maxLength={100}
                disabled={busy || mergeTarget !== ''}
                placeholder={editingSpeaker ? defaultSpeakerLabel(editingSpeaker) : ''}
                onChange={(e) => setNameDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleSave();
                  }
                }}
                autoFocus
              />
              <p className="text-xs text-gray-500">Leave empty to use the default label.</p>
            </div>

            {speakers.length > 1 && (
              <div className="space-y-1">
                <label htmlFor="speaker-merge" className="text-sm font-medium text-gray-700">Merge into</label>
                <select
                  id="speaker-merge"
                  value={mergeTarget}
                  disabled={busy}
                  onChange={(e) => setMergeTarget(e.target.value)}
                  className="w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-blue-500"
                >
                  <option value="">Don&apos;t merge</option>
                  {speakers
                    .filter((id) => id !== editingSpeaker)
                    .map((id) => (
                      <option key={id} value={id}>
                        {speakerDisplayName(id, speakerNames)}
                      </option>
                    ))}
                </select>
              </div>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={close} disabled={busy}>Cancel</Button>
            <Button onClick={() => void handleSave()} disabled={busy}>
              {mergeTarget ? 'Merge' : 'Save'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
