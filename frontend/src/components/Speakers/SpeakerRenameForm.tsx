'use client';

import { useState } from 'react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { toast } from 'sonner';

interface SpeakerRenameFormProps {
  speakerKey: string;
  /** Current custom name; the draft starts from it each time the form mounts (popover opens). */
  initialName: string;
  placeholder: string;
  label?: string;
  onRename: (key: string, name: string) => Promise<void>;
  onSaved: () => void;
}

export function SpeakerRenameForm({ speakerKey, initialName, placeholder, label, onRename, onSaved }: SpeakerRenameFormProps) {
  const [draft, setDraft] = useState(initialName);
  const fields = (
    <>
      <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={placeholder} autoFocus />
      <Button type="submit" size="sm">Save</Button>
    </>
  );
  return (
    <form
      className={label ? 'space-y-1' : 'flex gap-2'}
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          await onRename(speakerKey, draft);
          onSaved();
        } catch (error) {
          console.error('Failed to rename speaker', error);
          toast.error('Failed to rename speaker');
        }
      }}
    >
      {label ? (
        <>
          <label className="text-xs font-medium text-gray-600">{label}</label>
          <div className="flex gap-2">{fields}</div>
        </>
      ) : fields}
    </form>
  );
}
