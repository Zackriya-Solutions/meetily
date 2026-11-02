import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { JSDOM } from 'jsdom';

let dom;
let schema;
let parseFromClipboard;
const globals = new Map();

beforeAll(async () => {
  dom = new JSDOM('<!doctype html><html><body></body></html>');
  for (const key of ['window', 'document', 'navigator']) {
    globals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, {
      configurable: true,
      value: dom.window[key === 'window' ? 'window' : key],
    });
  }

  // Use Tiptap's entry points, matching the application's single-instance aliases.
  const { Schema } = await import('@tiptap/pm/model');
  ({ __parseFromClipboard: parseFromClipboard } = await import('@tiptap/pm/view'));
  schema = new Schema({
    nodes: {
      doc: { content: 'block+' },
      paragraph: {
        content: 'inline*', group: 'block',
        parseDOM: [{ tag: 'p' }], toDOM: () => ['p', 0],
      },
      blockquote: {
        content: 'block+', group: 'block',
        attrs: { label: { default: 'safe', validate: 'string' } },
        parseDOM: [{ tag: 'blockquote' }],
        toDOM: node => ['blockquote', { 'data-label': node.attrs.label }, 0],
      },
      text: { group: 'inline' },
    },
  });
});

afterAll(() => {
  dom?.window.close();
  for (const [key, descriptor] of globals) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor);
    else delete globalThis[key];
  }
});

function pasteWithContext(label) {
  const wrapper = document.createElement('div');
  wrapper.setAttribute('data-pm-slice', `0 0 ${JSON.stringify(['blockquote', { label }])}`);
  wrapper.innerHTML = '<p>Meeting notes</p>';
  const doc = schema.node('doc', null, [schema.node('paragraph')]);
  const view = { state: { schema }, someProp: () => undefined };
  return parseFromClipboard(view, '', wrapper.outerHTML, false, doc.resolve(1));
}

describe('clipboard slice context (GHSA-c8x8-7fp4-3x9w)', () => {
  test('rejects invalid context attributes without discarding pasted text', () => {
    const slice = pasteWithContext({ unexpected: 'object' });
    expect(slice.content.firstChild.type.name).toBe('paragraph');
    expect(slice.content.firstChild.textContent).toBe('Meeting notes');
  });

  test('preserves valid context attributes and pasted text', () => {
    const slice = pasteWithContext('meeting');
    expect(slice.content.firstChild.type.name).toBe('blockquote');
    expect(slice.content.firstChild.attrs.label).toBe('meeting');
    expect(slice.content.firstChild.textContent).toBe('Meeting notes');
  });
});
