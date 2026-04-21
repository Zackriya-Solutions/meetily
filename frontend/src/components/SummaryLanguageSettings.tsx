"use client";

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

const LANGUAGE_OPTIONS: { code: string; label: string }[] = [
  { code: "en-GB", label: "English (British)" },
  { code: "en-US", label: "English (American)" },
  { code: "zh", label: "Chinese" },
  { code: "de", label: "German" },
  { code: "es", label: "Spanish" },
  { code: "ru", label: "Russian" },
  { code: "ko", label: "Korean" },
  { code: "fr", label: "French" },
  { code: "ja", label: "Japanese" },
  { code: "pt", label: "Portuguese" },
  { code: "pt-BR", label: "Portuguese (Brazilian)" },
  { code: "it", label: "Italian" },
  { code: "nl", label: "Dutch" },
  { code: "pl", label: "Polish" },
  { code: "ar", label: "Arabic" },
  { code: "hi", label: "Hindi" },
  { code: "ta", label: "Tamil" },
  { code: "tr", label: "Turkish" },
  { code: "vi", label: "Vietnamese" },
  { code: "th", label: "Thai" },
  { code: "id", label: "Indonesian" },
  { code: "sv", label: "Swedish" },
];

const AUTO_VALUE = "__auto__";

export function SummaryLanguageSettings() {
  const [selected, setSelected] = useState<string>(AUTO_VALUE);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const stored = (await invoke("api_get_summary_language")) as
          | string
          | null;
        setSelected(stored ?? AUTO_VALUE);
      } catch (err) {
        console.error("Failed to load summary language:", err);
        toast.error("Failed to load summary language");
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const handleChange = async (value: string) => {
    const previous = selected;
    setSelected(value);
    try {
      await invoke("api_set_summary_language", {
        language: value === AUTO_VALUE ? null : value,
      });
      toast.success(
        value === AUTO_VALUE
          ? "Summary language reset to automatic"
          : `Summaries will be generated in ${
              LANGUAGE_OPTIONS.find((l) => l.code === value)?.label ?? value
            }`,
      );
    } catch (err) {
      console.error("Failed to save summary language:", err);
      toast.error("Failed to save summary language");
      setSelected(previous);
    }
  };

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
      <h3 className="text-lg font-semibold mb-2">Summary Language</h3>
      <p className="text-sm text-gray-600 mb-4">
        Choose the language used for generated meeting summaries. Leave as
        automatic to let the model match the transcript language.
      </p>

      <label htmlFor="summary-language-select" className="sr-only">
        Summary language
      </label>
      <select
        id="summary-language-select"
        value={selected}
        onChange={(e) => handleChange(e.target.value)}
        disabled={loading}
        className="block w-full max-w-sm rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-50"
      >
        <option value={AUTO_VALUE}>Automatic (match transcript)</option>
        {LANGUAGE_OPTIONS.map((opt) => (
          <option key={opt.code} value={opt.code}>
            {opt.label} ({opt.code})
          </option>
        ))}
      </select>
    </div>
  );
}
