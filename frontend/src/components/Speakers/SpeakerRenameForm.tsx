'use client';

import { useState } from 'react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { toast } from 'sonner';
import type { Person } from '@/types';
import { matchPeople } from '@/lib/people';

interface SpeakerRenameFormProps {
  speakerKey: string;
  /** Current custom name; the draft starts from it each time the form mounts (popover opens). */
  initialName: string;
  placeholder: string;
  label?: string;
  /** Known people offered while typing; picking one names the speaker with that person's stored name. */
  people: Person[];
  onRename: (key: string, name: string) => Promise<void>;
  onSaved: () => void;
}

export function SpeakerRenameForm({ speakerKey, initialName, placeholder, label, people, onRename, onSaved }: SpeakerRenameFormProps) {
  const [draft, setDraft] = useState(initialName);
  // Nothing is offered until the draft differs from the current name.
  const matches = draft.trim() === initialName.trim() ? [] : matchPeople(people, draft);
  const save = async (name: string) => {
    try {
      await onRename(speakerKey, name);
      onSaved();
    } catch (error) {
      console.error('Failed to rename speaker', error);
      // Refusals from the backend (for example while a speaker job runs) are written for the user.
      toast.error(typeof error === 'string' ? error : 'Failed to rename speaker');
    }
  };
  const fields = (
    <>
      <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={placeholder} autoFocus />
      <Button type="submit" size="sm">Save</Button>
    </>
  );
  return (
    <div className="space-y-1">
      <form
        className={label ? 'space-y-1' : 'flex gap-2'}
        onSubmit={async (e) => {
          e.preventDefault();
          await save(draft);
        }}
      >
        {label ? (
          <>
            <label className="text-xs font-medium text-gray-600">{label}</label>
            <div className="flex gap-2">{fields}</div>
          </>
        ) : fields}
      </form>
      {matches.length > 0 && (
        <div className="space-y-0.5">
          {matches.map((person) => (
            <button
              key={person.id}
              type="button"
              aria-label={`Name as ${person.name}`}
              className="flex w-full items-center justify-between rounded px-2 py-1 text-left text-sm hover:bg-gray-100"
              onClick={() => save(person.name)}
            >
              <span className="truncate">{person.name}</span>
              <span className="ml-2 shrink-0 text-xs text-gray-400">
                {`${person.meeting_count} meeting${person.meeting_count === 1 ? '' : 's'}`}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
