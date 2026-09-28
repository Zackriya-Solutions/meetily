'use client';

import { memo, useState } from 'react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { MeetingSpeaker } from '@/types';
import { speakerColor, speakerLabel } from '@/lib/speakers';
import { toast } from 'sonner';

interface SpeakerChipProps {
  speakerKey: string;
  transcriptId: string;
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  editable: boolean;
  /** Small dot trigger for rows inside a same-speaker run; renders nothing when not editable. */
  compact?: boolean;
  onRename: (key: string, name: string) => Promise<void>;
  onMerge: (fromKey: string, intoKey: string) => Promise<void>;
  onReassign: (transcriptId: string, key: string | null) => Promise<void>;
}

function SpeakerChipImpl({
  speakerKey,
  transcriptId,
  speakers,
  names,
  editable,
  compact = false,
  onRename,
  onMerge,
  onReassign,
}: SpeakerChipProps) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState('');
  const label = speakerLabel(speakerKey, names);
  const color = speakerColor(speakerKey);
  if (compact && !editable) return null;
  const chip = (
    <span className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium ${color.chip}`}>{label}</span>
  );
  if (!editable) return chip;

  // Speakers left without rows (every line reassigned away) are not offered.
  const others = speakers.filter((s) => s.speaker_key !== speakerKey && s.row_count > 0);
  const current = speakers.find((s) => s.speaker_key === speakerKey);
  const run = async (action: () => Promise<void>, failure: string) => {
    try {
      await action();
      setOpen(false);
    } catch (error) {
      console.error(failure, error);
      toast.error(failure);
    }
  };

  return (
    <Popover open={open} onOpenChange={(next) => { setOpen(next); if (next) setDraft(current?.display_name ?? ''); }}>
      <PopoverTrigger asChild>
        {compact ? (
          <button
            type="button"
            aria-label={`Change speaker (${label})`}
            title="Change speaker"
            className={`h-3 w-3 rounded-full ${color.dot}`}
          />
        ) : (
          <button type="button" className="cursor-pointer" title="Edit speaker">{chip}</button>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-64 space-y-3" align="start">
        <form
          className="space-y-1"
          onSubmit={(e) => { e.preventDefault(); void run(() => onRename(speakerKey, draft), 'Failed to rename speaker'); }}
        >
          <label className="text-xs font-medium text-gray-600">Name for everyone labelled {label}</label>
          <div className="flex gap-2">
            <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={label} autoFocus />
            <Button type="submit" size="sm">Save</Button>
          </div>
        </form>
        {others.length > 0 && (
          <div className="space-y-1">
            <div className="text-xs font-medium text-gray-600">Same person as…</div>
            {others.map((s) => {
              const otherLabel = speakerLabel(s.speaker_key, names);
              return (
                <button
                  key={s.speaker_key}
                  type="button"
                  aria-label={`Same person as ${otherLabel}`}
                  className="block w-full rounded px-2 py-1 text-left text-sm hover:bg-gray-100"
                  onClick={() => void run(() => onMerge(speakerKey, s.speaker_key), 'Failed to merge speakers')}
                >
                  {otherLabel}
                </button>
              );
            })}
          </div>
        )}
        <div className="space-y-1">
          <div className="text-xs font-medium text-gray-600">This line was said by…</div>
          {others.map((s) => {
            const otherLabel = speakerLabel(s.speaker_key, names);
            return (
              <button
                key={s.speaker_key}
                type="button"
                aria-label={`This line was said by ${otherLabel}`}
                className="block w-full rounded px-2 py-1 text-left text-sm hover:bg-gray-100"
                onClick={() => void run(() => onReassign(transcriptId, s.speaker_key), 'Failed to change speaker')}
              >
                {otherLabel}
              </button>
            );
          })}
          <button
            type="button"
            aria-label="This line was said by a new speaker"
            className="block w-full rounded px-2 py-1 text-left text-sm text-gray-600 hover:bg-gray-100"
            onClick={() => void run(() => onReassign(transcriptId, null), 'Failed to change speaker')}
          >
            New speaker
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** Memoised: every prop is a primitive or a stable reference, so scrolling does not re-render it. */
export const SpeakerChip = memo(SpeakerChipImpl);
