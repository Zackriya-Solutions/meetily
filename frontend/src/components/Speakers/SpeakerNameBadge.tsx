'use client';

import { Check, X } from 'lucide-react';
import type { SpeakerNameState } from '@/lib/speakers';

interface SpeakerNameBadgeProps {
  state: SpeakerNameState;
  /** The chip's label: the name, or "Speaker N" while only a suggestion exists. */
  label: string;
  editable: boolean;
  onConfirm: () => void;
  onReject: () => void;
}

const ACTION = 'inline-flex h-5 w-5 items-center justify-center rounded text-gray-500 hover:bg-gray-100 hover:text-gray-900';

/** Sits next to a chip (never inside its popover trigger): the "auto" mark or the suggested
 *  name, with confirm and "Not <name>" while editing is possible. */
export function SpeakerNameBadge({ state, label, editable, onConfirm, onReject }: SpeakerNameBadgeProps) {
  if (state.kind !== 'auto' && state.kind !== 'suggestion') return null;
  const { name } = state;
  return (
    <span className="inline-flex items-center gap-0.5 text-xs">
      {state.kind === 'auto' ? (
        <span
          className="rounded bg-gray-100 px-1 text-[10px] font-medium uppercase tracking-wide text-gray-500"
          title={state.source === 'voice' ? 'Recognised by voice' : 'Found in the conversation'}
        >
          auto
        </span>
      ) : (
        <span className="text-gray-500" title={state.reason ?? undefined} aria-label={`${label} might be ${name}`}>
          {`· ${name}?`}
        </span>
      )}
      {editable && (
        <>
          <button type="button" className={ACTION} aria-label={`Confirm ${name}`} title={`Confirm ${name}`} onClick={onConfirm}>
            <Check className="h-3 w-3" />
          </button>
          <button type="button" className={ACTION} aria-label={`Not ${name}`} title={`Not ${name}`} onClick={onReject}>
            <X className="h-3 w-3" />
          </button>
        </>
      )}
    </span>
  );
}
