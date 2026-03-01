import { invoke } from '@tauri-apps/api/core';

export interface OutlineConfig {
  url: string;
  apiKey: string;
  collectionId: string;
}

const OUTLINE_CONFIG_KEY = 'outline_config';

export function loadOutlineConfig(): OutlineConfig {
  if (typeof window === 'undefined') return { url: '', apiKey: '', collectionId: '' };
  try {
    const raw = localStorage.getItem(OUTLINE_CONFIG_KEY);
    if (raw) return JSON.parse(raw) as OutlineConfig;
  } catch {
    // ignore
  }
  return { url: '', apiKey: '', collectionId: '' };
}

export function saveOutlineConfig(config: OutlineConfig): void {
  localStorage.setItem(OUTLINE_CONFIG_KEY, JSON.stringify(config));
}

export async function exportToOutline(
  config: OutlineConfig,
  title: string,
  markdown: string,
): Promise<string> {
  const baseUrl = config.url.replace(/\/$/, '');
  console.log('[Outline] exportToOutline → base_url:', baseUrl, 'collection:', config.collectionId);

  try {
    const docUrl = await invoke<string>('outline_create_document', {
      baseUrl,
      apiKey: config.apiKey,
      collectionId: config.collectionId,
      title,
      text: markdown,
    });
    console.log('[Outline] exportToOutline success:', docUrl);
    return docUrl;
  } catch (e) {
    console.error('[Outline] exportToOutline failed:', e);
    throw new Error(typeof e === 'string' ? e : (e as Error).message ?? String(e));
  }
}

/** Fetch collections list to let users pick a destination */
export async function fetchOutlineCollections(
  config: Pick<OutlineConfig, 'url' | 'apiKey'>,
): Promise<Array<{ id: string; name: string }>> {
  const baseUrl = config.url.replace(/\/$/, '');
  console.log('[Outline] fetchOutlineCollections → base_url:', baseUrl);

  try {
    const collections = await invoke<Array<{ id: string; name: string }>>('outline_fetch_collections', {
      baseUrl,
      apiKey: config.apiKey,
    });
    console.log('[Outline] fetchOutlineCollections success, count:', collections.length);
    return collections;
  } catch (e) {
    console.error('[Outline] fetchOutlineCollections failed:', e);
    throw new Error(typeof e === 'string' ? e : (e as Error).message ?? String(e));
  }
}
