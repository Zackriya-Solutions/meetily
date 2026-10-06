import { afterAll, afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { PlaybackSource } from '../../src/types';
import type { PlaybackControls } from '../../src/hooks/usePlayback';
import { FORCE_CLIP_KEY } from '../../src/lib/playback';

type Listener = () => void;
const audios: FakeAudio[] = [];
/** Just enough of HTMLAudioElement: tests set currentTime/error and emit the events. */
class FakeAudio {
  static canPlay = 'maybe';
  src = '';
  currentTime = 0;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  paused = true;
  ended = false;
  preload = '';
  error: { code: number } | null = null;
  private listeners = new Map<string, Set<Listener>>();
  constructor() { audios.push(this); }
  canPlayType() { return FakeAudio.canPlay; }
  addEventListener(type: string, listener: Listener) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(listener);
  }
  removeEventListener(type: string, listener: Listener) { this.listeners.get(type)?.delete(listener); }
  removeAttribute(name: string) { if (name === 'src') this.src = ''; }
  load() {}
  play() {
    this.paused = false;
    this.ended = false;
    this.emit('play');
    return Promise.resolve();
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.emit('pause');
  }
  emit(type: string) { for (const listener of [...(this.listeners.get(type) ?? [])]) listener(); }
}
Object.defineProperty(globalThis, 'Audio', { configurable: true, value: FakeAudio });

const originalCore = { ...await import('@tauri-apps/api/core') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  Reflect.deleteProperty(globalThis, 'Audio');
});
/** What the backend sends: container time is the recording clock, so the table is the identity. */
const IDENTITY_SOURCE: PlaybackSource = {
  url: 'asset://localhost/%2Frecordings%2Fmeeting%2Faudio.mp4',
  duration_s: 60,
  time_table: [[0, 0], [60, 60]],
};
let source: PlaybackSource = IDENTITY_SOURCE;
const fullClip = () => new ArrayBuffer(44 + 30 * 16_000 * 2);
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_prepare_meeting_playback') return source;
  if (command === 'api_render_playback_clip') return fullClip();
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ ...originalCore, invoke }));
const { usePlayback, CLIP_SEEK_DELAY_MS } = await import('../../src/hooks/usePlayback');

let controls: PlaybackControls;
function Player() {
  controls = usePlayback('meeting-a', true);
  return null;
}
let renderer: ReactTestRenderer | undefined;
async function settle() {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}
async function mount() {
  await act(async () => { renderer = create(<Player />); });
  await settle();
  return audios[audios.length - 1];
}
const clipStarts = () => invoke.mock.calls
  .filter(([command]) => command === 'api_render_playback_clip')
  .map(([, args]) => (args as { startFileS: number }).startFileS);
const near = (actual: number, expected: number) => expect(Math.abs(actual - expected)).toBeLessThan(1e-9);

beforeEach(() => {
  audios.length = 0;
  FakeAudio.canPlay = 'maybe';
  source = IDENTITY_SOURCE;
  invoke.mockClear();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  Reflect.deleteProperty(globalThis, 'localStorage');
  Reflect.deleteProperty(globalThis, 'document');
});

describe('usePlayback', () => {
  test('prepares the source and plays from a clock time', async () => {
    const audio = await mount();
    expect(invoke).toHaveBeenCalledWith('api_prepare_meeting_playback', { meetingId: 'meeting-a' });
    expect(controls.ready).toBe(true);
    expect(controls.mode).toBe('asset');
    expect(controls.durationS).toBe(60);
    expect(audio.src).toBe(source.url);
    await act(async () => { controls.playFrom(45); });
    near(audio.currentTime, 45);
    expect(audio.paused).toBe(false);
    expect(controls.playing).toBe(true);
    audio.currentTime = 50;
    await act(async () => { audio.emit('timeupdate'); });
    near(controls.clockS, 50);
  });

  test('positions go through the time table the backend sends', async () => {
    source = { ...IDENTITY_SOURCE, time_table: [[0, 0.02], [30, 30.02], [30, 30.06], [60, 60.06]] };
    const audio = await mount();
    await act(async () => { controls.playFrom(45); });
    near(audio.currentTime, 45.06);
    audio.currentTime = 50.06;
    await act(async () => { audio.emit('timeupdate'); });
    near(controls.clockS, 50);
  });

  test('switches to clip mode on src not supported', async () => {
    const audio = await mount();
    await act(async () => { controls.playFrom(45); });
    audio.error = { code: 4 };
    await act(async () => { audio.emit('error'); });
    await settle();
    expect(controls.mode).toBe('clip');
    expect(clipStarts()).toHaveLength(1);
    near(clipStarts()[0], 45);
    expect(invoke).toHaveBeenCalledWith('api_render_playback_clip', expect.objectContaining({ meetingId: 'meeting-a', seconds: 30 }));
    expect(audio.src.startsWith('blob:')).toBe(true);
    expect(audio.paused).toBe(false);
  });

  test('forced clip mode from local storage', async () => {
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true, value: { getItem: (key: string) => (key === FORCE_CLIP_KEY ? '1' : null) },
    });
    const audio = await mount();
    expect(controls.mode).toBe('clip');
    expect(controls.ready).toBe(true);
    expect(audio.src).toBe('');
  });

  test('clip mode prefetches the next clip', async () => {
    FakeAudio.canPlay = '';
    const audio = await mount();
    expect(controls.mode).toBe('clip');
    await act(async () => { controls.playFrom(0); });
    await settle();
    near(clipStarts()[0], 0);
    const first = audio.src;
    expect(first.startsWith('blob:')).toBe(true);
    audio.currentTime = 26;
    await act(async () => { audio.emit('timeupdate'); });
    await settle();
    expect(clipStarts()).toHaveLength(2);
    near(clipStarts()[1], 30);
    audio.ended = true;
    await act(async () => { audio.emit('ended'); });
    await settle();
    expect(audio.src).not.toBe(first);
    expect(clipStarts()).toHaveLength(2); // the prefetched clip was used
    expect(controls.playing).toBe(true);
  });

  test('dragging the seek bar in clip mode renders one clip where the drag stops', async () => {
    FakeAudio.canPlay = '';
    const audio = await mount();
    await act(async () => { controls.playFrom(0); });
    await settle();
    const before = clipStarts().length;
    await act(async () => {
      controls.seek(40);
      controls.seek(45);
      controls.seek(50);
    });
    near(controls.clockS, 50);
    expect(clipStarts()).toHaveLength(before); // nothing rendered while the drag goes on
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, CLIP_SEEK_DELAY_MS + 50)); });
    await settle();
    expect(clipStarts()).toHaveLength(before + 1);
    near(clipStarts()[before], 50);
    expect(audio.paused).toBe(false);
  });

  test('play from with stop pauses at the end', async () => {
    const audio = await mount();
    await act(async () => { controls.playFrom(10, 18); });
    near(audio.currentTime, 10);
    audio.currentTime = 17.5;
    await act(async () => { audio.emit('timeupdate'); });
    expect(audio.paused).toBe(false);
    audio.currentTime = 18.2;
    await act(async () => { audio.emit('timeupdate'); });
    expect(audio.paused).toBe(true);
    expect(controls.playing).toBe(false);
  });

  test('speed changes playback rate', async () => {
    const audio = await mount();
    await act(async () => { controls.setRate(1.5); });
    expect(audio.playbackRate).toBe(1.5);
    expect(controls.rate).toBe(1.5);
  });

  test('Space toggles playback, also on the seek bar and on buttons, but not in a text field', async () => {
    const keyListeners = new Set<(event: unknown) => void>();
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: {
        addEventListener: (type: string, listener: (event: unknown) => void) => { if (type === 'keydown') keyListeners.add(listener); },
        removeEventListener: (_type: string, listener: (event: unknown) => void) => { keyListeners.delete(listener); },
      },
    });
    const press = async (target: object) => {
      let prevented = false;
      const event = { code: 'Space', key: ' ', repeat: false, defaultPrevented: false, target, preventDefault: () => { prevented = true; } };
      await act(async () => { keyListeners.forEach((listener) => listener(event)); });
      return prevented;
    };
    const audio = await mount();
    expect(await press({ tagName: 'INPUT', type: 'range', closest: () => null })).toBe(true);
    expect(audio.paused).toBe(false);
    expect(await press({ tagName: 'BUTTON', closest: () => null })).toBe(true);
    expect(audio.paused).toBe(true);
    expect(await press({ tagName: 'INPUT', type: 'text', closest: () => null })).toBe(false);
    expect(audio.paused).toBe(true);
  });
});
