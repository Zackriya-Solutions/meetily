"use client";

import { useEffect, useState } from 'react';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

interface MeetingAudioPlayerProps {
  meetingId: string;
}

/** Plays the meeting's saved recording; renders nothing when none was saved. */
export function MeetingAudioPlayer({ meetingId }: MeetingAudioPlayerProps) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    invoke<string | null>('get_meeting_audio_path', { meetingId })
      .then(path => {
        if (!cancelled && path) setSrc(convertFileSrc(path));
      })
      .catch(error => console.error('Failed to find the meeting recording:', error));
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  if (!src) return null;
  // Linux WebKitGTK cannot always stream from the asset scheme
  // (tauri-apps/tauri#3725); the file itself is still in the folder.
  if (failed) {
    return (
      <p className="mt-3 text-sm text-gray-500">
        The recording can&apos;t play here. Use Recording to open its folder.
      </p>
    );
  }
  return (
    <audio
      controls
      preload="metadata"
      src={src}
      onError={() => setFailed(true)}
      className="mt-3 w-full h-9"
    />
  );
}
