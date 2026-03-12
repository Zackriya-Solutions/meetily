'use client';

import { useEffect, useMemo, useState } from 'react';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { authFetch } from '@/lib/api';
import { toast } from 'sonner';

const TEMPLATE_SKILLS: Record<string, string> = {
  facilitator: `# SKILL: Meeting Facilitator

## Role & Identity
You are a neutral meeting facilitator focused on progress, clarity, and inclusion.

## Behavior Rules
- Keep the group aligned to agenda and outcomes.
- Encourage participation from quieter attendees.
- Intervene politely when discussions stall.

## Policy Config
\`\`\`yaml
role_mode: facilitator
min_confidence: 0.70
suggestion_cooldown_seconds: 45
intervention_cooldown_seconds: 120
allow_interruptions: false
threshold_decision_candidate: 0.72
threshold_conflict_risk: 0.70
threshold_agenda_drift: 0.68
threshold_urgency_risk: 0.72
threshold_mistake_candidate: 0.80
threshold_unheard_participant: 0.78
threshold_open_question: 0.70
forbidden_actions: shame_participants, legal_advice
\`\`\``,
  advisor: `# SKILL: Strategic Advisor

## Role & Identity
You are an advisor who intervenes selectively and only on high-signal risks.

## Behavior Rules
- Stay mostly quiet unless risk is material.
- Prefer strong evidence before intervening.
- Focus on urgency, conflict, and factual correction.

## Policy Config
\`\`\`yaml
role_mode: advisor
min_confidence: 0.78
suggestion_cooldown_seconds: 90
intervention_cooldown_seconds: 180
allow_interruptions: false
threshold_decision_candidate: 0.80
threshold_conflict_risk: 0.78
threshold_agenda_drift: 0.76
threshold_urgency_risk: 0.80
threshold_mistake_candidate: 0.85
threshold_unheard_participant: 0.84
threshold_open_question: 0.80
forbidden_actions: shame_participants, legal_advice
\`\`\``,
  chairperson: `# SKILL: Chairperson

## Role & Identity
You are a chairperson focused on keeping decisions timely and discussions productive.

## Behavior Rules
- Drive topic transitions when time is constrained.
- Push for concrete decision closure.
- Surface unresolved blockers quickly.

## Policy Config
\`\`\`yaml
role_mode: chairperson
min_confidence: 0.65
suggestion_cooldown_seconds: 35
intervention_cooldown_seconds: 90
allow_interruptions: false
threshold_decision_candidate: 0.68
threshold_conflict_risk: 0.66
threshold_agenda_drift: 0.64
threshold_urgency_risk: 0.65
threshold_mistake_candidate: 0.76
threshold_unheard_participant: 0.74
threshold_open_question: 0.66
forbidden_actions: shame_participants, legal_advice
\`\`\``,
};

interface MeetingSkillResponse {
  meeting_id: string;
  skill_markdown: string;
  is_active: boolean;
  source: string;
}

interface MeetingAIHostSkillDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  meetingId: string;
}

export function MeetingAIHostSkillDialog({ open, onOpenChange, meetingId }: MeetingAIHostSkillDialogProps) {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [isActive, setIsActive] = useState(true);
  const [skillMarkdown, setSkillMarkdown] = useState('');

  const lineCount = useMemo(() => skillMarkdown.split('\n').length, [skillMarkdown]);

  const loadMeetingProfile = async () => {
    if (!meetingId) return;
    setLoading(true);
    try {
      const res = await authFetch(`/meeting-ai-host-skill/${meetingId}`, { method: 'GET' });
      if (!res.ok) {
        if (res.status === 403) {
          toast.error('You do not have permission to view meeting AI host skill');
          return;
        }
        throw new Error('Failed to load meeting AI host skill');
      }
      const data = (await res.json()) as MeetingSkillResponse;
      setSkillMarkdown(data.skill_markdown || '');
      setIsActive(Boolean(data.is_active));
    } catch (error) {
      console.error(error);
      toast.error('Failed to load meeting AI host skill');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    void loadMeetingProfile();
  }, [open, meetingId]);

  const applyTemplate = (name: keyof typeof TEMPLATE_SKILLS) => {
    setSkillMarkdown(TEMPLATE_SKILLS[name]);
  };

  const saveMeetingProfile = async () => {
    if (!meetingId) return;
    setSaving(true);
    try {
      const res = await authFetch('/meeting-ai-host-skill', {
        method: 'POST',
        body: JSON.stringify({
          meeting_id: meetingId,
          skill_markdown: skillMarkdown,
          is_active: isActive,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.detail || 'Failed to save meeting AI host skill');
      }
      toast.success('Meeting AI host skill saved');
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to save meeting AI host skill');
    } finally {
      setSaving(false);
    }
  };

  const deleteMeetingProfile = async () => {
    if (!meetingId) return;
    setDeleting(true);
    try {
      const res = await authFetch(`/meeting-ai-host-skill/${meetingId}`, { method: 'DELETE' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.detail || 'Failed to delete meeting AI host skill');
      }
      setSkillMarkdown('');
      setIsActive(true);
      toast.success('Meeting AI host skill deleted');
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to delete meeting AI host skill');
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>Meeting AI Host Skill</DialogTitle>
          <DialogDescription>
            This profile applies only to this meeting and overrides your user default profile for this meeting.
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="text-sm text-gray-600">Loading...</div>
        ) : (
          <div className="space-y-4">
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={() => applyTemplate('facilitator')}
                className="rounded border border-gray-300 bg-gray-50 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-100"
              >
                Facilitator
              </button>
              <button
                type="button"
                onClick={() => applyTemplate('advisor')}
                className="rounded border border-gray-300 bg-gray-50 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-100"
              >
                Advisor
              </button>
              <button
                type="button"
                onClick={() => applyTemplate('chairperson')}
                className="rounded border border-gray-300 bg-gray-50 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-100"
              >
                Chairperson
              </button>
            </div>

            <label className="inline-flex items-center gap-2 text-sm text-gray-700">
              <input
                type="checkbox"
                checked={isActive}
                onChange={(e) => setIsActive(e.target.checked)}
                className="h-4 w-4 rounded border-gray-300"
              />
              Active for this meeting
            </label>

            <textarea
              value={skillMarkdown}
              onChange={(e) => setSkillMarkdown(e.target.value)}
              placeholder="role_mode: facilitator\nmin_confidence: 0.70\n..."
              className="w-full min-h-[280px] rounded-md border border-gray-300 p-3 text-sm font-mono focus:border-gray-500 focus:outline-none"
            />

            <div className="flex items-center justify-between">
              <div className="text-xs text-gray-500">
                {lineCount} lines · {skillMarkdown.length} chars · Max 20000 chars
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={deleteMeetingProfile}
                  disabled={deleting || saving}
                  className="rounded border border-red-300 bg-red-50 px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-100 disabled:opacity-50"
                >
                  {deleting ? 'Deleting...' : 'Delete'}
                </button>
                <button
                  type="button"
                  onClick={saveMeetingProfile}
                  disabled={saving || deleting || skillMarkdown.length > 20000}
                  className="rounded bg-gray-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-gray-800 disabled:opacity-50"
                >
                  {saving ? 'Saving...' : 'Save'}
                </button>
              </div>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
