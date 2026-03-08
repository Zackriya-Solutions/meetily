'use client';

import { useEffect } from 'react';
import Analytics from '@/lib/analytics';

interface NotePageViewTrackerProps {
  noteId: string;
  noteTitle: string;
}

export function NotePageViewTracker({ noteId, noteTitle }: NotePageViewTrackerProps) {
  useEffect(() => {
    Analytics.track('page_view_note', {
      note_id: noteId,
      note_title: noteTitle,
    }).catch(console.error);
  }, [noteId, noteTitle]);

  return null;
}

