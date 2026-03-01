"use client";

import { useEffect, useState } from "react";
import { ExternalLink, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  OutlineConfig,
  fetchOutlineCollections,
  loadOutlineConfig,
  saveOutlineConfig,
} from "@/lib/outlineExport";

export function OutlineSettings() {
  const [config, setConfig] = useState<OutlineConfig>({
    url: "",
    apiKey: "",
    collectionId: "",
  });
  const [collections, setCollections] = useState<Array<{ id: string; name: string }>>([]);
  const [isFetching, setIsFetching] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    setConfig(loadOutlineConfig());
  }, []);

  const handleChange = (field: keyof OutlineConfig, value: string) => {
    setConfig((prev) => ({ ...prev, [field]: value }));
    setSaved(false);
  };

  const handleFetchCollections = async () => {
    setFetchError(null);

    // Basic URL validation
    let normalizedUrl = config.url.trim();
    if (!normalizedUrl.startsWith('http://') && !normalizedUrl.startsWith('https://')) {
      normalizedUrl = 'https://' + normalizedUrl;
      setConfig((prev) => ({ ...prev, url: normalizedUrl }));
    }

    console.log('[OutlineSettings] Fetching collections from:', normalizedUrl);
    setIsFetching(true);
    try {
      const cols = await fetchOutlineCollections({ url: normalizedUrl, apiKey: config.apiKey });
      console.log('[OutlineSettings] Got collections:', cols);
      setCollections(cols);
      if (cols.length > 0 && !config.collectionId) {
        setConfig((prev) => ({ ...prev, collectionId: cols[0].id }));
      }
      if (cols.length === 0) {
        setFetchError('No collections found. Check your API key has the right permissions.');
      }
    } catch (e: any) {
      const msg = e?.message ?? String(e);
      console.error('[OutlineSettings] fetchOutlineCollections failed:', msg);
      setFetchError(msg);
    } finally {
      setIsFetching(false);
    }
  };

  const handleSave = () => {
    saveOutlineConfig(config);
    setSaved(true);
  };

  return (
    <div className="mt-6 space-y-6 max-w-xl">
      <div>
        <h2 className="text-lg font-semibold mb-1">Outline Integration</h2>
        <p className="text-sm text-gray-500">
          Export meeting summaries directly to your{" "}
          <a
            href="https://www.getoutline.com"
            target="_blank"
            rel="noreferrer"
            className="text-blue-600 hover:underline inline-flex items-center gap-0.5"
          >
            Outline <ExternalLink className="w-3 h-3" />
          </a>{" "}
          knowledge base.
        </p>
      </div>

      <div className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="outline-url">Outline URL</Label>
          <Input
            id="outline-url"
            placeholder="https://app.getoutline.com"
            value={config.url}
            onChange={(e) => handleChange("url", e.target.value)}
          />
          <p className="text-xs text-gray-400">
            Use <code>https://app.getoutline.com</code> for cloud, or your self-hosted URL.
          </p>
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="outline-api-key">API Key</Label>
          <Input
            id="outline-api-key"
            type="password"
            placeholder="ol_api_…"
            value={config.apiKey}
            onChange={(e) => handleChange("apiKey", e.target.value)}
          />
          <p className="text-xs text-gray-400">
            Create an API token in Outline → Settings → API Tokens.
          </p>
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="outline-collection">Export Collection</Label>
          <div className="flex gap-2">
            {collections.length > 0 ? (
              <select
                id="outline-collection"
                className="flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                value={config.collectionId}
                onChange={(e) => handleChange("collectionId", e.target.value)}
              >
                {collections.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            ) : (
              <Input
                id="outline-collection"
                placeholder="Collection ID (or load from API)"
                value={config.collectionId}
                onChange={(e) => handleChange("collectionId", e.target.value)}
                className="flex-1"
              />
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={handleFetchCollections}
              disabled={!config.url || !config.apiKey || isFetching}
              title="Load collections from your Outline instance"
            >
              {isFetching ? <Loader2 className="w-4 h-4 animate-spin" /> : "Load"}
            </Button>
          </div>
          {fetchError && <p className="text-xs text-red-500">{fetchError}</p>}
          <p className="text-xs text-gray-400">
            Click <strong>Load</strong> to fetch your collections, then pick one. Or paste a
            Collection ID directly.
          </p>
        </div>
      </div>

      <Button onClick={handleSave} disabled={!config.url || !config.apiKey || !config.collectionId}>
        {saved ? "Saved ✓" : "Save Settings"}
      </Button>
    </div>
  );
}

