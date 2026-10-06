import { describe, expect, test } from 'bun:test';
import { errorMessage } from '../../src/lib/errors';

describe('errorMessage', () => {
  test('backend refusals arrive as strings and are shown as written', () => {
    expect(errorMessage('A person named Noah already exists', 'Failed to rename')).toBe('A person named Noah already exists');
  });

  test('an Error shows its message; anything else shows the fallback', () => {
    expect(errorMessage(new Error('Network down'), 'Failed to rename')).toBe('Network down');
    expect(errorMessage({ code: 1 }, 'Failed to rename')).toBe('Failed to rename');
    expect(errorMessage(undefined, 'Failed to rename')).toBe('Failed to rename');
  });
});
