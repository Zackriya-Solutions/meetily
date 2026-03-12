'use client';

import { useEffect, useMemo, useState } from 'react';
import { authFetch } from '@/lib/api';
import { toast } from 'sonner';

interface StyleItem {
  id: string;
  name: string;
  source: 'system' | 'user';
  read_only: boolean;
  is_default: boolean;
  is_active: boolean;
  skill_markdown: string;
}

interface StylesResponse {
  styles: StyleItem[];
  default_style_id: string;
}

export function AIHostSkillSettings() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [styles, setStyles] = useState<StyleItem[]>([]);
  const [defaultStyleId, setDefaultStyleId] = useState('system:facilitator');
  const [selectedStyleId, setSelectedStyleId] = useState('system:facilitator');
  const [draftName, setDraftName] = useState('');
  const [draftMarkdown, setDraftMarkdown] = useState('');
  const [draftActive, setDraftActive] = useState(true);
  const [isCreating, setIsCreating] = useState(false);
  const [askBeforeMeeting, setAskBeforeMeeting] = useState<boolean>(() => {
    if (typeof window === 'undefined') return false;
    return localStorage.getItem('ai_host_ask_before_meeting') === 'true';
  });

  const selectedStyle = useMemo(
    () => styles.find((style) => style.id === selectedStyleId) || null,
    [styles, selectedStyleId]
  );

  const loadStyles = async () => {
    setLoading(true);
    try {
      const res = await authFetch('/api/user/ai-host-styles', { method: 'GET' });
      if (!res.ok) throw new Error('Failed to load AI Participant styles');
      const data = (await res.json()) as StylesResponse;
      setStyles(data.styles || []);
      setDefaultStyleId(data.default_style_id || 'system:facilitator');
      const selected = data.styles?.some((s) => s.id === data.default_style_id)
        ? data.default_style_id
        : (data.styles?.[0]?.id || 'system:facilitator');
      setSelectedStyleId(selected);
      setIsCreating(false);
    } catch (error) {
      console.error(error);
      toast.error('Failed to load AI Participant styles');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadStyles();
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    localStorage.setItem('ai_host_ask_before_meeting', askBeforeMeeting ? 'true' : 'false');
  }, [askBeforeMeeting]);

  useEffect(() => {
    if (!selectedStyle || isCreating) return;
    setDraftName(selectedStyle.name || '');
    setDraftMarkdown(selectedStyle.skill_markdown || '');
    setDraftActive(Boolean(selectedStyle.is_active));
  }, [selectedStyle, isCreating]);

  const startCreate = () => {
    setIsCreating(true);
    setDraftName('My Custom Participant Style');
    setDraftMarkdown(`---\nname: "My Custom Participant"\ndescription: "A focused AI participant tuned for my team."\n---\n\n# Role\nYou are the AI Participant for this meeting. Keep the team focused and surface concrete actions.\n\n# Goals\n1. Capture explicit decisions only when the group clearly agrees.\n2. Surface unresolved discussion that still needs closure.\n3. Flag the team-specific signals that matter most.\n\n# Allowed Custom Event Types\n- \`blocker_detected\`: When someone cannot proceed without help.\n- \`scope_creep\`: When new work appears outside the active goal.\n- \`owner_missing\`: When work is discussed without a clear owner.\n\n# Rules\n- Be concise and evidence-based.\n- Do not invent facts.\n- Prefer actionable observations over commentary.\n\n\`\`\`yaml\nrole_mode: facilitator\nmin_confidence: 0.70\nsuggestion_cooldown_seconds: 45\nintervention_cooldown_seconds: 120\nallow_interruptions: false\nthreshold_decision_candidate: 0.72\nthreshold_open_discussion: 0.70\nthreshold_blocker_detected: 0.70\nforbidden_actions: shame_participants, legal_advice\n\`\`\``);
    setDraftActive(true);
  };

  const save = async () => {
    if (draftMarkdown.length > 20000) {
      toast.error('Skill markdown exceeds max length (20000)');
      return;
    }
    setSaving(true);
    try {
      if (isCreating) {
        const res = await authFetch('/api/user/ai-host-styles', {
          method: 'POST',
          body: JSON.stringify({
            name: draftName,
            skill_markdown: draftMarkdown,
            is_active: draftActive,
            set_default: false,
          }),
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          throw new Error(body?.detail || 'Failed to create style');
        }
        toast.success('Custom AI Participant style created');
      } else if (selectedStyle && selectedStyle.source === 'user') {
        const res = await authFetch(`/api/user/ai-host-styles/${selectedStyle.id}`, {
          method: 'PUT',
          body: JSON.stringify({
            name: draftName,
            skill_markdown: draftMarkdown,
            is_active: draftActive,
          }),
        });
        if (!res.ok) {
          const body = await res.json().catch(() => ({}));
          throw new Error(body?.detail || 'Failed to update style');
        }
        toast.success('Custom AI Participant style updated');
      }
      await loadStyles();
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to save style');
    } finally {
      setSaving(false);
    }
  };

  const setDefault = async () => {
    const styleId = isCreating ? '' : (selectedStyle?.id || '');
    if (!styleId) {
      toast.error('Please select a style first');
      return;
    }
    setSaving(true);
    try {
      const res = await authFetch('/api/user/ai-host-styles/default', {
        method: 'POST',
        body: JSON.stringify({ style_id: styleId }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.detail || 'Failed to set default style');
      }
      toast.success('Default AI Participant style updated');
      await loadStyles();
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to set default style');
    } finally {
      setSaving(false);
    }
  };

  const deleteSelected = async () => {
    if (!selectedStyle || selectedStyle.source !== 'user') return;
    setSaving(true);
    try {
      const res = await authFetch(`/api/user/ai-host-styles/${selectedStyle.id}`, {
        method: 'DELETE',
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.detail || 'Failed to delete style');
      }
      toast.success('Custom AI Participant style deleted');
      await loadStyles();
    } catch (error) {
      console.error(error);
      toast.error(error instanceof Error ? error.message : 'Failed to delete style');
    } finally {
      setSaving(false);
    }
  };

  const readOnly = !isCreating && Boolean(selectedStyle?.read_only);

  if (loading) {
    return <div className="text-sm text-gray-600">Loading AI Participant styles...</div>;
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm space-y-4">
        <div>
          <h3 className="text-lg font-semibold text-gray-900">AI Participant Style Library</h3>
          <p className="text-sm text-gray-600 mt-1">
            System styles are read-only. Create custom styles for your team and set one as default for quick start.
          </p>
        </div>

        <label className="inline-flex items-center gap-2 text-sm text-gray-700">
          <input
            type="checkbox"
            checked={askBeforeMeeting}
            onChange={(e) => setAskBeforeMeeting(e.target.checked)}
            className="h-4 w-4 rounded border-gray-300"
          />
          Ask style before every meeting start
        </label>

        <div className="flex flex-wrap items-center gap-2">
          <select
            value={isCreating ? '__creating__' : selectedStyleId}
            onChange={(e) => {
              const next = e.target.value;
              if (next === '__creating__') {
                startCreate();
                return;
              }
              setIsCreating(false);
              setSelectedStyleId(next);
            }}
            className="rounded border border-gray-300 px-3 py-2 text-sm"
          >
            {styles.map((style) => (
              <option key={style.id} value={style.id}>
                {style.is_default ? '★ ' : ''}{style.name} ({style.source})
              </option>
            ))}
            <option value="__creating__">+ New Custom Style</option>
          </select>

          <button
            type="button"
            onClick={setDefault}
            disabled={saving || isCreating}
            className="rounded border border-blue-300 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100 disabled:opacity-50"
          >
            Set As Default
          </button>

          {selectedStyle?.source === 'user' && !isCreating && (
            <button
              type="button"
              onClick={deleteSelected}
              disabled={saving}
              className="rounded border border-red-300 bg-red-50 px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-100 disabled:opacity-50"
            >
              Delete Style
            </button>
          )}
        </div>

        <div className="text-xs text-gray-500">
          Current default: <span className="font-medium">{defaultStyleId}</span>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium text-gray-600">Style name</label>
          <input
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            disabled={readOnly}
            className="w-full rounded border border-gray-300 px-3 py-2 text-sm disabled:bg-gray-100 disabled:text-gray-500"
          />
        </div>

        <label className="inline-flex items-center gap-2 text-sm text-gray-700">
          <input
            type="checkbox"
            checked={draftActive}
            onChange={(e) => setDraftActive(e.target.checked)}
            disabled={readOnly}
            className="h-4 w-4 rounded border-gray-300"
          />
          Active style
        </label>

        <textarea
          value={draftMarkdown}
          onChange={(e) => setDraftMarkdown(e.target.value)}
          disabled={readOnly}
          className="w-full min-h-[320px] rounded-md border border-gray-300 p-3 text-sm font-mono focus:border-gray-500 focus:outline-none disabled:bg-gray-100 disabled:text-gray-500"
        />

        <div className="flex items-center justify-between">
          <div className="text-xs text-gray-500">
            {draftMarkdown.split('\n').length} lines · {draftMarkdown.length} chars · Max 20000 chars
          </div>
          <div className="flex items-center gap-2">
            {isCreating && (
              <button
                type="button"
                onClick={() => {
                  setIsCreating(false);
                  setSelectedStyleId(defaultStyleId || 'system:facilitator');
                }}
                className="rounded border border-gray-300 bg-gray-50 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-100"
              >
                Cancel
              </button>
            )}
            <button
              type="button"
              onClick={save}
              disabled={saving || readOnly || draftMarkdown.length > 20000 || !draftName.trim()}
              className="rounded bg-gray-900 px-3 py-1.5 text-xs font-medium text-white hover:bg-gray-800 disabled:opacity-50"
            >
              {saving ? 'Saving...' : isCreating ? 'Create Style' : 'Save Changes'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
