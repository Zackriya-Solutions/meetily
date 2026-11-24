'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface AccuracyCounts {
  scored: number;
  correct: number;
  unlabelled: number;
  /** Lines whose voice was named from an earlier meeting. */
  guessed: number;
}

interface SpeakerAccuracyResult {
  overall: AccuracyCounts;
  meetings: Record<string, AccuracyCounts>;
}

interface SpeakerAccuracyProps {
  meetingId: string;
  /** Changes whenever names are saved, so the score is read again. */
  refreshKey: number;
}

function percent(c: AccuracyCounts): string {
  return `${((c.correct / c.scored) * 100).toFixed(1)}%`;
}

/**
 * How often the automatic speaker labels match the names given by hand,
 * counting lines of 2.5 s or more. Hidden until something has been named.
 */
export function SpeakerAccuracy({ meetingId, refreshKey }: SpeakerAccuracyProps) {
  const [result, setResult] = useState<SpeakerAccuracyResult | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<SpeakerAccuracyResult>('get_speaker_accuracy')
      .then(r => { if (!cancelled) setResult(r); })
      .catch(error => console.error('Failed to read speaker accuracy:', error));
    return () => { cancelled = true; };
  }, [meetingId, refreshKey]);

  const here = result?.meetings[meetingId];
  if (!result || result.overall.scored === 0) return null;

  return (
    <p className="mt-2 text-xs text-gray-500">
      Speaker labels match your names on{' '}
      {here && here.scored > 0 && (
        <>{percent(here)} of {here.scored} named lines here, </>
      )}
      {percent(result.overall)} of {result.overall.scored} across all meetings
      {result.overall.unlabelled > 0 && ` (${result.overall.unlabelled} had no label)`}.
      {result.overall.guessed > 0 && ` ${result.overall.guessed} were named automatically from earlier meetings.`}
    </p>
  );
}
