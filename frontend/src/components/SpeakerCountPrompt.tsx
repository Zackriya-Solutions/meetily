'use client';

import { useEffect, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from './ui/dialog';
import { Button } from './ui/button';
import { Input } from './ui/input';

interface SpeakerCountPromptProps {
  open: boolean;
  defaultValue: number;
  onConfirm: (count: number) => void;
  onCancel: () => void;
}

export function SpeakerCountPrompt({
  open,
  defaultValue,
  onConfirm,
  onCancel,
}: SpeakerCountPromptProps) {
  const [value, setValue] = useState(String(defaultValue));

  // Reset value when dialog opens
  useEffect(() => {
    if (open) {
      setValue(String(defaultValue));
    }
  }, [open, defaultValue]);

  const handleConfirm = () => {
    let count = parseInt(value, 10);

    // Validation: empty, NaN, <1 → 2; >20 → 20
    if (isNaN(count) || count < 1) {
      count = 2;
    } else if (count > 20) {
      count = 20;
    }

    onConfirm(count);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleConfirm();
    } else if (e.key === 'Escape') {
      onCancel();
    }
  };

  return (
    <Dialog open={open} onOpenChange={(isOpen) => {
      if (!isOpen) onCancel();
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>How many speakers?</DialogTitle>
          <DialogDescription>
            Enter the expected number of speakers for this recording. This helps label who said what in transcripts.
          </DialogDescription>
        </DialogHeader>

        <div className="py-4">
          <Input
            type="number"
            min="1"
            max="20"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={handleKeyDown}
            autoFocus
            placeholder="2"
            className="w-full"
          />
          <p className="text-xs text-gray-500 mt-2">
            Valid range: 1–20 speakers. Empty or invalid input defaults to 2.
          </p>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={handleConfirm}>
            Confirm
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
