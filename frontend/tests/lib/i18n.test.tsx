import { afterEach, describe, expect, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import i18n, { getPreferredUiLanguage, setUiLanguage, UI_LANGUAGE_STORAGE_KEY } from '../../src/i18n';
import { languageLabel, uiLabel, uiText } from '../../src/i18n/ui';
import { editorDictionary } from '../../src/i18n/editor';
import { ChunkProgressDisplay } from '../../src/components/ChunkProgressDisplay';
import { getModelTagline, MODEL_CONFIGS } from '../../src/lib/whisper';
import en from '../../src/i18n/locales/en-US.json';
import zh from '../../src/i18n/locales/zh-CN.json';

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) act(() => renderer!.unmount());
  renderer = undefined;
  await i18n.changeLanguage('en-US');
});

function leaves(value: Record<string, unknown>, prefix = ''): Record<string, string> {
  return Object.fromEntries(Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return typeof child === 'string' ? [[path, child]] : Object.entries(leaves(child as Record<string, unknown>, path));
  }));
}

describe('interface localization', () => {
  test('English and Chinese have matching keys and interpolation variables', () => {
    const english = leaves(en), chinese = leaves(zh);
    expect(Object.keys(chinese).sort()).toEqual(Object.keys(english).sort());
    for (const [key, value] of Object.entries(english)) {
      expect(chinese[key].trim().length).toBeGreaterThan(0);
      const placeholders = (text: string) => [...text.matchAll(/\{\{(\w+)\}\}/g)].map(match => match[1]).sort();
      expect(placeholders(chinese[key])).toEqual(placeholders(value));
    }
  });

  test('a mounted interface switches both ways without translating meeting content', async () => {
    act(() => { renderer = create(<ChunkProgressDisplay progress={{ total_chunks: 1, completed_chunks: 1, processing_chunks: 0, failed_chunks: 0, chunks: [{ chunk_id: 1, status: 'completed', text_preview: "Customer's original English meeting content" }] }} />); });
    expect(JSON.stringify(renderer!.toJSON())).toContain('Processing Progress');
    await act(async () => { await i18n.changeLanguage('zh-CN'); });
    const chinese = JSON.stringify(renderer!.toJSON());
    expect(chinese).toContain('处理进度');
    expect(chinese).toContain("Customer's original English meeting content");
    await act(async () => { await i18n.changeLanguage('en-US'); });
    expect(JSON.stringify(renderer!.toJSON())).toContain('Processing Progress');
  });

  test('model metadata and language labels change while protocol identifiers stay stable', async () => {
    await i18n.changeLanguage('zh-CN');
    expect(languageLabel('de', 'German')).toBe('德语');
    expect(languageLabel('auto', 'Auto Detect (Original Language)')).toBe('自动检测（原始语言）');
    expect(uiLabel('Qwen 3.5 2B (Balanced)')).toBe('Qwen 3.5 2B（均衡）');
    expect(MODEL_CONFIGS['large-v3'].speed).toBe('Slow');
    expect(MODEL_CONFIGS['large-v3'].description).toContain('准确率');
    expect(getModelTagline('large-v3', 'Slow', 'High')).toContain('较慢处理');
    expect(uiText('messages.transcriptSegmentsSaved', { value0: 7 })).toBe('已保存 7 个转录片段。');
    expect(uiText('messages.failedToStartRecordingNN', { value0: 'device detail' })).toContain('device detail');
  });

  test('editor menu localization keeps the same dictionary reference', async () => {
    const dictionary = editorDictionary;
    expect(dictionary.slash_menu.paragraph.title).toBe('Paragraph');
    await i18n.changeLanguage('zh-CN');
    expect(editorDictionary).toBe(dictionary);
    expect(dictionary.slash_menu.paragraph.title).toBe('段落');
  });

  test('the interface language choice is remembered independently of transcription language', async () => {
    const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
    const previousDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
    const storage = new Map<string, string>([['selectedLanguage', 'ja']]);
    const html = { lang: 'en-US' };
    Object.defineProperty(globalThis, 'window', { configurable: true, value: {
      localStorage: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) },
      navigator: { language: 'en-US' },
    } });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: { documentElement: html } });
    try {
      await setUiLanguage('zh-CN');
      expect(storage.get(UI_LANGUAGE_STORAGE_KEY)).toBe('zh-CN');
      expect(getPreferredUiLanguage()).toBe('zh-CN');
      expect(html.lang).toBe('zh-CN');
      expect(storage.get('selectedLanguage')).toBe('ja');
    } finally {
      if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow); else Reflect.deleteProperty(globalThis, 'window');
      if (previousDocument) Object.defineProperty(globalThis, 'document', previousDocument); else Reflect.deleteProperty(globalThis, 'document');
    }
  });
});
