'use client';

import { useEffect } from 'react';
import { User, Loader2, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { toast } from 'sonner';
import { useSpeakers } from '@/contexts/SpeakerContext';
import { Input } from '@/components/ui/input';
import { useState } from 'react';
import { Check, X, Pencil } from 'lucide-react';

const PRESET_COLORS = [
  '#6366f1', '#f59e0b', '#10b981', '#ef4444', '#3b82f6',
  '#8b5cf6', '#ec4899', '#14b8a6', '#f97316', '#84cc16',
];

interface SpeakerManagerProps {
  meetingId: string;
  onSpeakerUpdated?: () => void;
}

export function SpeakerManager({ meetingId, onSpeakerUpdated }: SpeakerManagerProps) {
  const { resolvedSpeakers, suggestions, loading, renameSpeaker, detectNames, applySuggestion, dismissSuggestion, meetingId: contextMeetingId, setMeetingId } = useSpeakers();

  // Safety net: if context meetingId doesn't match (e.g. direct navigation),
  // sync it. page-content normally handles this on mount.
  useEffect(() => {
    if (contextMeetingId !== meetingId) {
      setMeetingId(meetingId);
    }
  }, [meetingId, contextMeetingId, setMeetingId]);

  const [editing, setEditing] = useState<Record<string, { name: string; color: string }>>({});
  const [saving, setSaving] = useState<Record<string, boolean>>({});
  const [detecting, setDetecting] = useState(false);

  const startEdit = (speakerId: string) => {
    const resolved = resolvedSpeakers.find(s => s.speaker_id === speakerId);
    setEditing(prev => ({
      ...prev,
      [speakerId]: { name: resolved?.display_name ?? '', color: resolved?.color ?? PRESET_COLORS[0] },
    }));
  };

  const cancelEdit = (speakerId: string) => {
    setEditing(prev => { const n = { ...prev }; delete n[speakerId]; return n; });
  };

  const saveEdit = async (speakerId: string) => {
    const edit = editing[speakerId];
    if (!edit?.name.trim()) return;
    setSaving(prev => ({ ...prev, [speakerId]: true }));
    try {
      await renameSpeaker(speakerId, edit.name.trim(), edit.color, speakerId === 'mic');
      cancelEdit(speakerId);
      onSpeakerUpdated?.();
    } finally {
      setSaving(prev => { const n = { ...prev }; delete n[speakerId]; return n; });
    }
  };

  const handleDetect = async () => {
    setDetecting(true);
    const found = await detectNames();
    setDetecting(false);
    if (!found?.length) toast.info('No names detected in transcript');
  };

  if (loading || contextMeetingId !== meetingId) {
    return (
      <div className="flex items-center gap-2 text-sm text-gray-500 py-2">
        <Loader2 className="w-4 h-4 animate-spin" /> Loading speakers…
      </div>
    );
  }

  if (resolvedSpeakers.length === 0) {
    return (
      <p className="text-sm text-gray-400 italic py-2">
        No speakers detected. Re-transcribe with "Identify speakers" enabled to detect speakers.
      </p>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-xs text-gray-500">
        Assign names to speakers. Names will appear in transcripts and summaries.
        Naming yourself here saves your identity for future meetings.
      </p>

      {/* Auto-detect button */}
      <Button
        variant="outline"
        size="sm"
        className="w-full gap-2 text-xs"
        onClick={handleDetect}
        disabled={detecting}
      >
        {detecting ? <Loader2 className="w-3 h-3 animate-spin" /> : <Sparkles className="w-3 h-3" />}
        Detect names from transcript
      </Button>

      {/* Auto-detected suggestions */}
      {suggestions.length > 0 && (
        <div className="rounded-lg border border-amber-100 bg-amber-50 p-2 space-y-1.5">
          <p className="text-[11px] font-semibold text-amber-700 uppercase tracking-wide">Detected names</p>
          {suggestions.map((s, i) => (
            <div key={i} className="flex items-center justify-between gap-2">
              <div className="flex-1 min-w-0">
                <span className="text-xs font-medium text-gray-800">{s.name}</span>
                <span className="text-xs text-gray-400 ml-1">
                  — {s.speaker_id === 'mic' ? 'You' : s.speaker_id.replace('speaker_', 'Speaker ')}
                </span>
                <span className="text-[10px] text-amber-600 block">{s.pattern}</span>
              </div>
              <div className="flex gap-1 flex-shrink-0">
                <Button size="sm" variant="ghost" className="h-6 w-6 p-0 text-green-600"
                  onClick={() => applySuggestion(s)}>
                  <Check className="w-3 h-3" />
                </Button>
                <Button size="sm" variant="ghost" className="h-6 w-6 p-0 text-red-400"
                  onClick={() => dismissSuggestion(s)}>
                  <X className="w-3 h-3" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Speaker list */}
      {resolvedSpeakers.map(speaker => {
        const isEditingThis = !!editing[speaker.speaker_id];
        const edit = editing[speaker.speaker_id];
        const isSaving = saving[speaker.speaker_id];
        const isMic = speaker.speaker_id === 'mic';

        return (
          <div key={speaker.speaker_id} className="p-2 rounded-lg border border-gray-100 hover:border-gray-200 transition-colors">
            {isEditingThis ? (
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
                    style={{ backgroundColor: edit.color }}>
                    <User className="w-3.5 h-3.5 text-white" />
                  </div>
                  <Input
                    value={edit.name}
                    onChange={e => setEditing(prev => ({ ...prev, [speaker.speaker_id]: { ...edit, name: e.target.value } }))}
                    className="h-7 text-sm flex-1"
                    placeholder="Enter name…"
                    onKeyDown={e => { if (e.key === 'Enter') saveEdit(speaker.speaker_id); if (e.key === 'Escape') cancelEdit(speaker.speaker_id); }}
                    autoFocus
                  />
                </div>
                <div className="flex items-center gap-1 pl-9">
                  <div className="flex gap-1 flex-1 flex-wrap">
                    {PRESET_COLORS.map(c => (
                      <button key={c}
                        className="w-5 h-5 rounded-full border-2 transition-transform hover:scale-110"
                        style={{ backgroundColor: c, borderColor: edit.color === c ? '#111' : 'transparent' }}
                        onClick={() => setEditing(prev => ({ ...prev, [speaker.speaker_id]: { ...edit, color: c } }))}
                      />
                    ))}
                  </div>
                  <Button size="sm" variant="ghost" className="h-7 w-7 p-0 text-green-600"
                    onClick={() => saveEdit(speaker.speaker_id)} disabled={isSaving}>
                    {isSaving ? <Loader2 className="w-3 h-3 animate-spin" /> : <Check className="w-3 h-3" />}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 w-7 p-0 text-red-400"
                    onClick={() => cancelEdit(speaker.speaker_id)}>
                    <X className="w-3 h-3" />
                  </Button>
                </div>
                {isMic && (
                  <p className="text-[10px] text-gray-400 pl-9">
                    This will be saved as your identity and applied to future meetings automatically.
                  </p>
                )}
              </div>
            ) : (
              <div className="flex items-center gap-3">
                <div className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0"
                  style={{ backgroundColor: speaker.color }}>
                  <User className="w-3.5 h-3.5 text-white" />
                </div>
                <div className="flex-1 min-w-0">
                  <span className="text-sm font-medium text-gray-800 truncate block">{speaker.display_name}</span>
                  <span className="text-xs text-gray-400">
                    {isMic ? 'Microphone (you)' : speaker.speaker_id === 'system' ? 'System audio' : speaker.speaker_id.replace('speaker_', 'Speaker ')}
                    {isMic && speaker.profile_id && <span className="ml-1 text-indigo-400">· saved identity</span>}
                  </span>
                </div>
                <Button size="sm" variant="ghost" className="h-7 w-7 p-0 text-gray-400 hover:text-gray-600"
                  onClick={() => startEdit(speaker.speaker_id)}>
                  <Pencil className="w-3 h-3" />
                </Button>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

