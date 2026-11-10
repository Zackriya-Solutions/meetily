import React from 'react';
import { afterAll, afterEach, expect, mock, setSystemTime, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

// Load originals before mocking, then load consumers after their native/UI boundaries are mocked.
const originals = await Promise.all([
  '@tauri-apps/api/core',
  '../../src/components/ui/dialog',
  '../../src/components/About',
  'next/image',
  'next/navigation',
  '../../src/contexts/RecordingStateContext',
  '../../src/lib/analytics',
].map(async path => [path, { ...await import(path) }] as const));
afterAll(() => { for (const [path, exports] of originals) mock.module(path, () => exports); });

let readDoodle = async (): Promise<string | null> => '/tmp/doodle.png';
const invoke = mock(async (command: string) => {
  if (command === 'doodle_sync') return readDoodle();
  if (command === 'api_get_meetings') return [];
  throw new Error(`Unexpected command: ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://localhost${path}`,
  invoke,
}));
const Container = ({ children }: { children?: React.ReactNode }) => <div>{children}</div>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: Container, DialogContent: Container, DialogTitle: Container, DialogTrigger: Container,
}));
mock.module('../../src/components/About', () => ({ About: () => null }));
mock.module('next/image', () => ({
  default: ({ priority, ...props }: React.ImgHTMLAttributes<HTMLImageElement> & { priority?: boolean }) => <img {...props} />,
}));
mock.module('next/navigation', () => ({ usePathname: () => '/', useRouter: () => ({}) }));
mock.module('../../src/contexts/RecordingStateContext', () => ({
  useRecordingState: () => ({ isRecording: false }),
}));
mock.module('../../src/lib/analytics', () => ({ default: { trackBackendConnection() {} } }));
const { default: Logo } = await import('../../src/components/Logo');
const { SidebarProvider, useSidebar } = await import('../../src/components/Sidebar/SidebarProvider');

function SidebarLogo() {
  const { doodlePath, doodleDate } = useSidebar();
  return <Logo isCollapsed doodlePath={doodlePath} doodleDate={doodleDate} />;
}

let renderer: ReactTestRenderer | undefined;
const originalDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  setSystemTime();
  if (originalDocument) Object.defineProperty(globalThis, 'document', originalDocument);
  else Reflect.deleteProperty(globalThis, 'document');
});

test('collapsed logo uses the bundled pencil without a doodle', async () => {
  await act(async () => { renderer = create(<Logo isCollapsed doodlePath={null} />); });
  expect(renderer!.root.findByType('img').props.src).toBe('/logo-collapsed.png');
  expect(JSON.stringify(renderer!.toJSON())).not.toContain('asset://');
});

test('collapsed logo shows the local doodle and falls back on image failure', async () => {
  await act(async () => { renderer = create(<Logo isCollapsed doodlePath="/tmp/doodle.png" doodleDate="2026-11-09" />); });
  const image = renderer!.root.findByType('img');
  expect(image.props.src).toMatch(/^asset:\/\/localhost\/tmp\/doodle\.png\?d=\d{4}-\d{2}-\d{2}$/);
  expect(image.props.alt).toBe('Meetily');
  await act(async () => image.props.onError());
  expect(renderer!.root.findByType('img').props.src).toBe('/logo-collapsed.png');
});

test('a new doodle path retries after an image failure', async () => {
  await act(async () => { renderer = create(<Logo isCollapsed doodlePath="/tmp/old-doodle.png" doodleDate="2026-11-09" />); });
  await act(async () => renderer!.root.findByType('img').props.onError());
  await act(async () => { renderer!.update(<Logo isCollapsed doodlePath="/tmp/new-doodle.png" doodleDate="2026-11-09" />); });
  expect(renderer!.root.findByType('img').props.src).toStartWith('asset://localhost/tmp/new-doodle.png?d=');
});

test('expanded logo keeps its text even when a doodle is available', async () => {
  await act(async () => { renderer = create(<Logo isCollapsed={false} doodlePath="/tmp/doodle.png" doodleDate="2026-11-09" />); });
  expect(renderer!.root.findAllByType('img')).toHaveLength(0);
  expect(renderer!.root.findByType('button').props['aria-label']).toBe('About Meetily');
  expect(renderer!.root.findByType('button').findByType('span').children).toEqual(['Meetily']);
});

for (const failedYesterday of [false, true]) {
  test(`next-day visibility refreshes the same path after sync completes (previous failure: ${failedYesterday})`, async () => {
    const page = Object.assign(new EventTarget(), { visibilityState: 'visible' });
    Object.defineProperty(globalThis, 'document', { configurable: true, value: page });
    setSystemTime(new Date('2026-11-09T12:00:00Z'));
    readDoodle = async () => '/tmp/doodle.png';
    invoke.mockClear();
    await act(async () => { renderer = create(<SidebarProvider><SidebarLogo /></SidebarProvider>); });
    expect(renderer!.root.findByType('img').props.src).toBe('asset://localhost/tmp/doodle.png?d=2026-11-09');
    if (failedYesterday) await act(async () => renderer!.root.findByType('img').props.onError());
    const previousSrc = renderer!.root.findByType('img').props.src;

    setSystemTime(new Date('2026-11-10T12:00:00Z'));
    page.visibilityState = 'hidden';
    await act(async () => { page.dispatchEvent(new Event('visibilitychange')); });
    expect(invoke.mock.calls.filter(([command]) => command === 'doodle_sync')).toHaveLength(1);
    const { promise, resolve: finishSync } = Promise.withResolvers<string | null>();
    readDoodle = () => promise;
    page.visibilityState = 'visible';
    await act(async () => { page.dispatchEvent(new Event('visibilitychange')); });
    expect(invoke.mock.calls.filter(([command]) => command === 'doodle_sync')).toHaveLength(2);
    expect(renderer!.root.findByType('img').props.src).toBe(previousSrc);
    await act(async () => { finishSync('/tmp/doodle.png'); });
    expect(renderer!.root.findByType('img').props.src).toBe('asset://localhost/tmp/doodle.png?d=2026-11-10');

    await act(async () => renderer!.unmount());
    renderer = undefined;
    page.dispatchEvent(new Event('visibilitychange'));
    expect(invoke.mock.calls.filter(([command]) => command === 'doodle_sync')).toHaveLength(2);
  });
}
