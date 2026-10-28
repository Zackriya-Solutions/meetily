import { describe, expect, test } from 'bun:test';
import { applicationKey, normalizeSourceOptions, selectedApplicationAvailable, sourceLabel } from '../../src/lib/recording-source';

describe('recording source preferences', () => {
  test('older preferences select all audio and enable the mic', () => {
    expect(normalizeSourceOptions()).toEqual({ source: { kind: 'system' }, microphoneEnabled: true });
  });
  test('app-only selection survives hydration and retains unavailable identity', () => {
    const source = { kind: 'application' as const, application: { bundleId: 'test.zoom', bundlePath: '/Applications/Zoom.app', name: 'Zoom' } };
    const options = normalizeSourceOptions({ source, microphoneEnabled: false });
    expect(options.microphoneEnabled).toBe(false);
    expect(options.source).toEqual(source);
    expect(selectedApplicationAvailable([], source)).toBe(false);
    expect(sourceLabel(options)).toBe('Zoom');
  });
  test('same name or bundle ID at a different installation is not the same app', () => {
    const app = { bundleId: 'test.zoom', bundlePath: '/Applications/Zoom.app', name: 'Zoom' };
    const other = { ...app, bundlePath: '/tmp/Zoom.app', audioActive: true };
    expect(applicationKey(app)).not.toBe(applicationKey(other));
    expect(selectedApplicationAvailable([other], { kind: 'application', application: app })).toBe(false);
  });
});
