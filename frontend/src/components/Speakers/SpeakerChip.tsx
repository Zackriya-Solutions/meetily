'use client';

import { memo, useState } from 'react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { MeetingSpeaker } from '@/types';
import { speakerColor, speakerLabel } from '@/lib/speakers';
import { toast } from 'sonner';
import { SpeakerRenameForm } from './SpeakerRenameForm';

interface SpeakerChipProps {
  speakerKey: string;
  transcriptId: string;
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  editable: boolean;
  /** Small dot trigger for rows inside a same-speaker run. */
  compact?: boolean;
  onRename: (key: string, name: string) => Promise<void>;
  onMerge: (fromKey: string, intoKey: string) => Promise<void>;
  onReassign: (transcriptId: string, key: string | null) => Promise<void>;
}

function SpeakerOptionButtons({ speakers, names, ariaPrefix, onPick }: {
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  ariaPrefix: string;
  onPick: (key: string) => void;
}) {
  return (
    <>
      {speakers.map((s) => {
        const otherLabel = speakerLabel(s.speaker_key, names);
        return (
          <button
            key={s.speaker_key}
            type="button"
            aria-label={`${ariaPrefix} ${otherLabel}`}
            className="block w-full rounded px-2 py-1 text-left text-sm hover:bg-gray-100"
            onClick={() => onPick(s.speaker_key)}
          >
            {otherLabel}
          </button>
        );
      })}
    </>
  );
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
  const label = speakerLabel(speakerKey, names);
  const color = speakerColor(speakerKey);
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
    <Popover open={open} onOpenChange={setOpen}>
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
        <SpeakerRenameForm
          speakerKey={speakerKey}
          initialName={current?.display_name ?? ''}
          placeholder={label}
          label={`Name for everyone labelled ${label}`}
          onRename={onRename}
          onSaved={() => setOpen(false)}
        />
        {others.length > 0 && (
          <div className="space-y-1">
            <div className="text-xs font-medium text-gray-600">Same person as…</div>
            <SpeakerOptionButtons
              speakers={others}
              names={names}
              ariaPrefix="Same person as"
              onPick={(key) => void run(() => onMerge(speakerKey, key), 'Failed to merge speakers')}
            />
          </div>
        )}
        <div className="space-y-1">
          <div className="text-xs font-medium text-gray-600">This line was said by…</div>
          <SpeakerOptionButtons
            speakers={others}
            names={names}
            ariaPrefix="This line was said by"
            onPick={(key) => void run(() => onReassign(transcriptId, key), 'Failed to change speaker')}
          />
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
