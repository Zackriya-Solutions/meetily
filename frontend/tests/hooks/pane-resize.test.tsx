import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import {
  clampValue,
  nextValueForKey,
  readStoredNumber,
  usePaneResize,
  type PaneResizeOptions,
} from '../../src/hooks/usePaneResize';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const store = new Map<string, string>();
const originalWindow = (globalThis as { window?: unknown }).window;
const originalStorage = (globalThis as { localStorage?: unknown }).localStorage;

beforeEach(() => {
  store.clear();
  (globalThis as { window?: unknown }).window = globalThis;
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => { store.set(key, value); },
  };
});

afterEach(() => {
  (globalThis as { window?: unknown }).window = originalWindow;
  (globalThis as { localStorage?: unknown }).localStorage = originalStorage;
});

type Hook = ReturnType<typeof usePaneResize>;

function renderHook(options: PaneResizeOptions) {
  const result: { current: Hook | null } = { current: null };
  function Probe() {
    result.current = usePaneResize(options);
    return null;
  }
  let renderer: ReactTestRenderer | null = null;
  act(() => { renderer = create(<Probe />); });
  return { result: result as { current: Hook }, unmount: () => act(() => renderer?.unmount()) };
}

const pointer = (clientX: number) => ({
  button: 0,
  clientX,
  pointerId: 1,
  preventDefault() {},
  currentTarget: { setPointerCapture() {} },
}) as never;

const key = (name: string) => ({ key: name, preventDefault() {} }) as never;

const base: PaneResizeOptions = {
  storageKey: 'test.width',
  defaultValue: 256,
  min: 200,
  max: 480,
  step: 16,
  valueFromPointer: (x) => x,
};

describe('pane resize helpers', () => {
  test('clamps into range', () => {
    expect(clampValue(100, 200, 480)).toBe(200);
    expect(clampValue(900, 200, 480)).toBe(480);
    expect(clampValue(300, 200, 480)).toBe(300);
  });

  test('ignores stored values that are missing, garbage or out of range', () => {
    expect(readStoredNumber('test.width', 200, 480, 256)).toBe(256);
    store.set('test.width', 'wide');
    expect(readStoredNumber('test.width', 200, 480, 256)).toBe(256);
    store.set('test.width', '999');
    expect(readStoredNumber('test.width', 200, 480, 256)).toBe(256);
    store.set('test.width', '320');
    expect(readStoredNumber('test.width', 200, 480, 256)).toBe(320);
  });

  test('maps keys to values, and ArrowLeft at the minimum to collapse only when allowed', () => {
    const range = { min: 200, max: 480, step: 16, canCollapse: false };
    expect(nextValueForKey('ArrowRight', 256, range)).toBe(272);
    expect(nextValueForKey('ArrowLeft', 256, range)).toBe(240);
    expect(nextValueForKey('Home', 256, range)).toBe(200);
    expect(nextValueForKey('End', 256, range)).toBe(480);
    expect(nextValueForKey('ArrowLeft', 200, range)).toBe(200);
    expect(nextValueForKey('ArrowLeft', 200, { ...range, canCollapse: true })).toBe('collapse');
    expect(nextValueForKey('Enter', 256, range)).toBeNull();
  });
});

describe('usePaneResize', () => {
  test('restores the stored width on mount', () => {
    store.set('test.width', '360');
    const { result, unmount } = renderHook(base);
    expect(result.current.value).toBe(360);
    unmount();
  });

  test('drags within the range and persists only on release', () => {
    const { result, unmount } = renderHook(base);
    act(() => result.current.handleProps.onPointerDown(pointer(256)));
    expect(result.current.isDragging).toBe(true);
    act(() => result.current.handleProps.onPointerMove(pointer(900)));
    expect(result.current.value).toBe(480);
    expect(store.get('test.width')).toBeUndefined();
    act(() => result.current.handleProps.onPointerMove(pointer(300)));
    act(() => result.current.handleProps.onPointerUp());
    expect(result.current.isDragging).toBe(false);
    expect(result.current.value).toBe(300);
    expect(store.get('test.width')).toBe('300');
    unmount();
  });

  test('a cancelled drag goes back to where it started', () => {
    const { result, unmount } = renderHook(base);
    act(() => result.current.handleProps.onPointerDown(pointer(256)));
    act(() => result.current.handleProps.onPointerMove(pointer(400)));
    act(() => result.current.handleProps.onPointerCancel());
    expect(result.current.value).toBe(256);
    expect(store.get('test.width')).toBeUndefined();
    unmount();
  });

  test('keyboard resizes and persists; double-click resets to the default', () => {
    const { result, unmount } = renderHook(base);
    act(() => result.current.handleProps.onKeyDown(key('End')));
    expect(result.current.value).toBe(480);
    expect(store.get('test.width')).toBe('480');
    act(() => result.current.handleProps.onKeyDown(key('ArrowLeft')));
    expect(result.current.value).toBe(464);
    act(() => result.current.handleProps.onDoubleClick());
    expect(result.current.value).toBe(256);
    expect(store.get('test.width')).toBe('256');
    unmount();
  });

  test('releasing a drag past collapseBelow collapses and keeps the earlier width', () => {
    let collapsed = 0;
    const { result, unmount } = renderHook({ ...base, collapseBelow: 120, onCollapse: () => { collapsed += 1; } });
    act(() => result.current.handleProps.onPointerDown(pointer(256)));
    act(() => result.current.handleProps.onPointerMove(pointer(150)));
    expect(result.current.collapseArmed).toBe(false);
    act(() => result.current.handleProps.onPointerMove(pointer(80)));
    expect(result.current.collapseArmed).toBe(true);
    expect(result.current.value).toBe(200);
    act(() => result.current.handleProps.onPointerUp());
    expect(collapsed).toBe(1);
    expect(result.current.collapseArmed).toBe(false);
    expect(result.current.value).toBe(256);
    expect(store.get('test.width')).toBeUndefined();
    unmount();
  });

  test('dragging back out of the collapse zone does not collapse', () => {
    let collapsed = 0;
    const { result, unmount } = renderHook({ ...base, collapseBelow: 120, onCollapse: () => { collapsed += 1; } });
    act(() => result.current.handleProps.onPointerDown(pointer(256)));
    act(() => result.current.handleProps.onPointerMove(pointer(80)));
    act(() => result.current.handleProps.onPointerMove(pointer(220)));
    act(() => result.current.handleProps.onPointerUp());
    expect(collapsed).toBe(0);
    expect(result.current.value).toBe(220);
    unmount();
  });

  test('ArrowLeft at the minimum collapses when a collapse handler is given', () => {
    let collapsed = 0;
    store.set('test.width', '200');
    const { result, unmount } = renderHook({ ...base, collapseBelow: 120, onCollapse: () => { collapsed += 1; } });
    act(() => result.current.handleProps.onKeyDown(key('ArrowLeft')));
    expect(collapsed).toBe(1);
    unmount();
  });
});
