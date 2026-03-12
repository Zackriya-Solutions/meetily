'use client';

import { useEffect, useMemo, useState } from 'react';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { authFetch } from '@/lib/api';
import { toast } from 'sonner';

const TEMPLATE_SKILLS: Record<string, string> = {
  facilitator: `---\nname: "Facilitator"\ndescription: "A neutral AI participant that keeps the meeting aligned and inclusive."\n---\n\n# Role\nYou are the AI Participant for this meeting. Act as a balanced facilitator who helps the group stay focused and move toward clear outcomes.\n\n# Goals\n1. Capture explicit decisions when the group clearly agrees.\n2. Surface unresolved discussion that still needs resolution.\n3. Highlight useful participant actions that move the meeting forward.\n\n# Allowed Custom Event Types\n- \`follow_up_needed\`: When a next step should be captured.\n- \`participation_gap\`: When an important voice is missing.\n- \`risk_signal\`: When a delivery or coordination risk is emerging.\n\n# Rules\n- Be concise and evidence-based.\n- Do not invent facts.\n- Prefer actionable observations.\n\n\`\`\`yaml\nrole_mode: facilitator\nmin_confidence: 0.70\nsuggestion_cooldown_seconds: 45\nintervention_cooldown_seconds: 120\nallow_interruptions: false\nthreshold_decision_candidate: 0.72\nthreshold_open_discussion: 0.70\nthreshold_follow_up_needed: 0.68\nforbidden_actions: shame_participants, legal_advice\n\`\`\``,
  advisor: `---\nname: "Advisor"\ndescription: "A selective AI participant that surfaces only high-signal strategic guidance."\n---\n\n# Role\nYou are the AI Participant for this meeting. Act as a strategic advisor who intervenes sparingly and only when the transcript shows a meaningful risk or opportunity.\n\n# Goals\n1. Capture explicit decisions with precision.\n2. Surface unresolved discussion before it is lost.\n3. Highlight high-signal participant actions backed by evidence.\n\n# Allowed Custom Event Types\n- \`tradeoff_warning\`: When a meaningful downside is being ignored.\n- \`priority_conflict\`: When competing priorities threaten execution.\n- \`stakeholder_risk\`: When alignment or buy-in seems weak.\n\n# Rules\n- Intervene selectively.\n- Favor stronger evidence over speculation.\n- Keep language direct and professional.\n\n\`\`\`yaml\nrole_mode: advisor\nmin_confidence: 0.78\nsuggestion_cooldown_seconds: 90\nintervention_cooldown_seconds: 180\nallow_interruptions: false\nthreshold_decision_candidate: 0.80\nthreshold_open_discussion: 0.80\nthreshold_tradeoff_warning: 0.78\nforbidden_actions: shame_participants, legal_advice\n\`\`\``,
  chairperson: `---\nname: "Chairperson"\ndescription: "A decisive AI participant focused on ownership, closure, and meeting control."\n---\n\n# Role\nYou are the AI Participant for this meeting. Act as a chairperson who pushes for closure and emphasizes accountability.\n\n# Goals\n1. Capture explicit decisions quickly.\n2. Surface open discussion that still needs closure.\n3. Highlight participant actions tied to ownership, timing, and scope.\n\n# Allowed Custom Event Types\n- \`owner_missing\`: When work lacks a clear owner.\n- \`deadline_risk\`: When timing commitments look weak.\n- \`scope_creep\`: When new work appears outside the active goal.\n\n# Rules\n- Be concise and decisive.\n- Push toward clarity and closure.\n- Avoid vague praise and personal criticism.\n\n\`\`\`yaml\nrole_mode: chairperson\nmin_confidence: 0.65\nsuggestion_cooldown_seconds: 35\nintervention_cooldown_seconds: 90\nallow_interruptions: false\nthreshold_decision_candidate: 0.68\nthreshold_open_discussion: 0.66\nthreshold_scope_creep: 0.64\nforbidden_actions: shame_participants, legal_advice\n\`\`\``,
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
          toast.error('You do not have permission to view meeting AI Participant skill');
          return;
        }
        throw new Error('Failed to load meeting AI Participant skill');
      }
      const data = (await res.json()) as MeetingSkillResponse;
      setSkillMarkdown(data.skill_markdown || '');
      setIsActive(Boolean(data.is_active));
    } catch (error) {
      console.error(error);
      toast.error('Failed to load meeting AI Participant skill');
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
        throw new Error(body?.detail || 'Failed to save meeting AI Participant skill');
      }
      toast.success('Meeting AI Participant skill saved');
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to save meeting AI Participant skill');
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
        throw new Error(body?.detail || 'Failed to delete meeting AI Participant skill');
      }
      setSkillMarkdown('');
      setIsActive(true);
      toast.success('Meeting AI Participant skill deleted');
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to delete meeting AI Participant skill');
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>Meeting AI Participant Skill</DialogTitle>
          <DialogDescription>
            This profile applies only to this meeting and overrides your default AI Participant style for this meeting.
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
              placeholder={'---\nname: "Custom Participant"\ndescription: "..."\n---\n\n# Role\n...\n'}
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
