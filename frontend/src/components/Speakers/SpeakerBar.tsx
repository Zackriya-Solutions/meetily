'use client';

import { useState } from 'react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { MeetingSpeaker } from '@/types';
import { speakerColor, speakerLabel } from '@/lib/speakers';
import { toast } from 'sonner';

interface SpeakerBarProps {
  speakers: MeetingSpeaker[];
  names: Record<string, string>;
  editable: boolean;
  onRename: (key: string, name: string) => Promise<void>;
}

function RenameButton({ speaker, names, onRename }: { speaker: MeetingSpeaker; names: Record<string, string>; onRename: SpeakerBarProps['onRename'] }) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState('');
  const label = speakerLabel(speaker.speaker_key, names);
  return (
    <Popover open={open} onOpenChange={(next) => { setOpen(next); if (next) setDraft(speaker.display_name ?? ''); }}>
      <PopoverTrigger asChild>
        <button type="button" className="hover:underline">{label}</button>
      </PopoverTrigger>
      <PopoverContent className="w-60" align="start">
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            try {
              await onRename(speaker.speaker_key, draft);
              setOpen(false);
            } catch (error) {
              console.error('Failed to rename speaker', error);
              toast.error('Failed to rename speaker');
            }
          }}
        >
          <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={label} autoFocus />
          <Button type="submit" size="sm">Save</Button>
        </form>
      </PopoverContent>
    </Popover>
  );
}

export function SpeakerBar({ speakers, names, editable, onRename }: SpeakerBarProps) {
  // Shares come from the rows each speaker has now, so reassignments show up; speakers left
  // without rows are hidden.
  const present = speakers.filter((s) => s.row_count > 0);
  if (present.length === 0) return null;
  const total = present.reduce((sum, s) => sum + s.row_seconds, 0) || 1;
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-gray-200 px-4 py-2 text-sm text-gray-700">
      {present.map((s) => (
        <span key={s.speaker_key} className="inline-flex items-center gap-1.5">
          <span className={`h-2 w-2 rounded-full ${speakerColor(s.speaker_key).dot}`} />
          {editable ? <RenameButton speaker={s} names={names} onRename={onRename} /> : speakerLabel(s.speaker_key, names)}
          <span className="text-gray-400">{Math.round((s.row_seconds / total) * 100)}%</span>
        </span>
      ))}
    </div>
  );
}
