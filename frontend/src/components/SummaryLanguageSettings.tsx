'use client';

import { useState } from 'react';
import { Globe, Pin } from 'lucide-react';
import { LanguagePickerPopover } from '@/components/LanguagePickerPopover';
import { useRecentLanguages } from '@/hooks/useRecentLanguages';
import { labelForCode } from '@/lib/summary-languages';

export function SummaryLanguageSettings() {
  const { recents, pinned, addRecent, removeRecent, setPinned } = useRecentLanguages();
  const [pickerOpen, setPickerOpen] = useState(false);

  const togglePin = (code: string) => {
    setPinned(pinned === code ? null : code);
  };

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm relative">
      <div className="flex items-center gap-2 mb-2">
        <Globe size={18} className="text-gray-500" />
        <h3 className="text-lg font-semibold text-gray-900">Summary Language</h3>
      </div>
      <p className="text-sm text-gray-600 mb-4">
        Pin one language as the default for new meetings. Unpinned languages remain as
        quick-switch options in the summary generator. Auto matches the transcription language.
      </p>

      <div className="flex flex-wrap items-center gap-2">
        {recents.map((code) => {
          const isPinned = pinned === code;
          return (
            <span
              key={code}
              className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm ${
                isPinned
                  ? 'bg-blue-50 border-blue-200 text-blue-800'
                  : 'bg-gray-100 border-gray-200 text-gray-800'
              }`}
            >
              <button
                type="button"
                aria-label={isPinned ? `Unpin ${labelForCode(code)} as default` : `Pin ${labelForCode(code)} as default`}
                aria-pressed={isPinned}
                title={isPinned ? 'Default language for new meetings' : 'Set as default for new meetings'}
                onClick={() => togglePin(code)}
                className={`leading-none ${isPinned ? 'text-blue-600' : 'text-gray-400 hover:text-gray-700'}`}
              >
                <Pin size={12} fill={isPinned ? 'currentColor' : 'none'} />
              </button>
              <span>{labelForCode(code)}</span>
              <button
                type="button"
                aria-label={`Remove ${labelForCode(code)}`}
                onClick={() => removeRecent(code)}
                className={`leading-none ${isPinned ? 'text-blue-400 hover:text-blue-700' : 'text-gray-400 hover:text-gray-700'}`}
              >
                ×
              </button>
            </span>
          );
        })}

        <button
          type="button"
          onClick={() => setPickerOpen((prev) => !prev)}
          disabled={recents.length >= 5}
          className="inline-flex items-center gap-1 rounded-full border border-dashed border-gray-300 px-3 py-1 text-sm text-gray-600 hover:border-gray-400 hover:text-gray-800 disabled:cursor-not-allowed disabled:opacity-50"
        >
          ＋ Add language
        </button>
      </div>

      <p className="text-xs text-gray-400 mt-3">
        {pinned
          ? `Default: ${labelForCode(pinned)}. Max 5 quick-switch options.`
          : 'No default pinned - new meetings use Auto. Max 5 quick-switch options.'}
      </p>

      {pickerOpen && (
        <div className="absolute z-10 mt-2">
          <LanguagePickerPopover
            mode="settings"
            value={null}
            onChange={(code) => {
              if (code) addRecent(code);
              setPickerOpen(false);
            }}
            onClose={() => setPickerOpen(false)}
          />
        </div>
      )}
    </div>
  );
}
