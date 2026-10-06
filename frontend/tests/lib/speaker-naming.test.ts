import { describe, expect, spyOn, test } from 'bun:test';
import {
  decideAutoGuessNames, formatNamingResult, hasUnnamedSpeaker, isLocalSummaryModel, isWaitingForSpeakers, shouldAutoGuessNames,
} from '../../src/lib/speakerNaming';
import { makeSpeaker } from '../fixtures/speakers';

const custom = (customOpenAIEndpoint: string | null) => ({ provider: 'custom-openai' as const, customOpenAIEndpoint });
const unnamed = [makeSpeaker('spk_0', { display_name: 'Noah', name_source: 'user' }), makeSpeaker('spk_1')];
const rule = (overrides: Partial<Parameters<typeof shouldAutoGuessNames>[0]> = {}) => shouldAutoGuessNames({
  speakerIdentification: true,
  modelConfig: { provider: 'claude', customOpenAIEndpoint: null },
  modelConfigLoaded: true,
  isAutoSummary: false,
  speakers: unnamed,
  ...overrides,
});

describe('local summary model', () => {
  test('ollama and the built-in model are local', () => {
    expect(isLocalSummaryModel({ provider: 'ollama', customOpenAIEndpoint: null })).toBe(true);
    expect(isLocalSummaryModel({ provider: 'builtin-ai' })).toBe(true);
  });

  test('a custom endpoint is local only on localhost or 127.0.0.1', () => {
    expect(isLocalSummaryModel(custom('http://localhost:8080'))).toBe(true);
    expect(isLocalSummaryModel(custom('http://127.0.0.1:1234/v1'))).toBe(true);
    expect(isLocalSummaryModel(custom('https://localhost.example.com/v1'))).toBe(false);
    expect(isLocalSummaryModel(custom('not a url'))).toBe(false);
    expect(isLocalSummaryModel(custom(null))).toBe(false);
  });

  test('cloud providers and a missing config are not local', () => {
    expect(isLocalSummaryModel({ provider: 'claude', customOpenAIEndpoint: 'http://localhost:8080' })).toBe(false);
    expect(isLocalSummaryModel(null)).toBe(false);
  });
});

describe('ollama endpoint', () => {
  const ollama = (ollamaEndpoint: string | null | undefined) => ({ provider: 'ollama' as const, ollamaEndpoint });

  test('a remote endpoint is not local', () => {
    expect(isLocalSummaryModel(ollama('http://192.168.1.20:11434'))).toBe(false);
    expect(isLocalSummaryModel(ollama('not a url'))).toBe(false);
  });

  test('the default and a loopback endpoint are local', () => {
    expect(isLocalSummaryModel(ollama(null))).toBe(true);
    expect(isLocalSummaryModel(ollama(undefined))).toBe(true);
    expect(isLocalSummaryModel(ollama(''))).toBe(true);
    expect(isLocalSummaryModel(ollama('http://localhost:11434'))).toBe(true);
    expect(isLocalSummaryModel(ollama('http://127.0.0.1:11434'))).toBe(true);
  });
});

describe('automatic naming decision from the saved config', () => {
  const input = {
    speakerIdentification: true, modelConfigLoaded: true, isAutoSummary: false, speakers: unnamed,
  };
  const saved = (provider: string) => async () => ({ provider, model: 'm', whisperModel: 'w' }) as never;

  test('the saved model decides, not an unsaved pick in the dialog', async () => {
    // The context may hold Ollama from an unsaved dialog pick while Claude is saved.
    expect(await decideAutoGuessNames({ ...input, readSavedModel: saved('claude') })).toBeNull();
    expect(await decideAutoGuessNames({ ...input, readSavedModel: saved('ollama') })).not.toBeNull();
  });

  test('the approval names the judged provider and endpoint', async () => {
    const withEndpoint = (extra: object) => async () => ({ provider: 'ollama', model: 'm', whisperModel: 'w', ...extra }) as never;
    expect(await decideAutoGuessNames({ ...input, readSavedModel: withEndpoint({ ollamaEndpoint: 'http://localhost:11434' }) }))
      .toEqual({ provider: 'ollama', endpoint: 'http://localhost:11434' });
    expect(await decideAutoGuessNames({ ...input, readSavedModel: withEndpoint({}) }))
      .toEqual({ provider: 'ollama', endpoint: null });
    const custom = async () => ({ provider: 'custom-openai', model: 'm', whisperModel: 'w', customOpenAIEndpoint: 'http://127.0.0.1:8000/v1' }) as never;
    expect(await decideAutoGuessNames({ ...input, readSavedModel: custom }))
      .toEqual({ provider: 'custom-openai', endpoint: 'http://127.0.0.1:8000/v1' });
  });

  test('a failed read counts as a cloud model', async () => {
    const logged = spyOn(console, 'error').mockImplementation(() => {});
    const failing = async () => { throw new Error('boom'); };
    expect(await decideAutoGuessNames({ ...input, readSavedModel: failing })).toBeNull();
    // Cloud is accepted by auto-summary; the read failed, so there is no judged model to pin.
    expect(await decideAutoGuessNames({ ...input, isAutoSummary: true, readSavedModel: failing })).toEqual({ provider: null, endpoint: null });
    logged.mockRestore();
  });

  test('the read is skipped when the config never loaded', async () => {
    expect(await decideAutoGuessNames({ ...input, modelConfigLoaded: false, readSavedModel: saved('ollama') })).toBeNull();
  });
});

describe('automatic naming rule', () => {
  test('a local model guesses names without auto-summary', () => {
    expect(rule({ modelConfig: { provider: 'ollama', customOpenAIEndpoint: null } })).toBe(true);
  });

  test('a cloud model guesses names only with auto-summary on', () => {
    expect(rule({ isAutoSummary: true })).toBe(true);
    expect(rule()).toBe(false);
  });

  test('a summary model not loaded counts as a cloud model', () => {
    // The config starts as Ollama and stays so while the saved one loads or when loading it fails;
    // that placeholder must not count as local, since the backend names with the saved model.
    expect(rule({ modelConfig: { provider: 'ollama', customOpenAIEndpoint: null }, modelConfigLoaded: false })).toBe(false);
    expect(rule({ modelConfigLoaded: false, isAutoSummary: true })).toBe(true);
  });

  test('nothing is sent with the beta off', () => {
    expect(rule({ speakerIdentification: false, isAutoSummary: true })).toBe(false);
  });

  test('nothing is sent when every speaker with rows has a name', () => {
    const named = [makeSpeaker('spk_0', { display_name: 'Noah' }), makeSpeaker('spk_1', { row_count: 0 })];
    expect(hasUnnamedSpeaker(named)).toBe(false);
    expect(hasUnnamedSpeaker(unnamed)).toBe(true);
    expect(rule({ speakers: named, isAutoSummary: true })).toBe(false);
  });
});

describe('naming result and summary wait', () => {
  test('result text', () => {
    expect(formatNamingResult(3, 2)).toBe('Named 3, suggested 2');
    expect(formatNamingResult(1, 0)).toBe('Named 1');
    expect(formatNamingResult(0, 2)).toBe('Suggested 2');
    expect(formatNamingResult(0, 0)).toBe('No names found');
  });

  test('the summary waits for the job and the pending automatic naming, up to the cap', () => {
    const base = { expired: false, statusKnown: true, isActive: false, autoNamingPending: false };
    expect(isWaitingForSpeakers(base)).toBe(false);
    expect(isWaitingForSpeakers({ ...base, statusKnown: false })).toBe(true);
    expect(isWaitingForSpeakers({ ...base, isActive: true })).toBe(true);
    expect(isWaitingForSpeakers({ ...base, autoNamingPending: true })).toBe(true);
    expect(isWaitingForSpeakers({ ...base, expired: true, isActive: true, autoNamingPending: true })).toBe(false);
  });
});
