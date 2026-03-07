/**
 * BlockNote theme objects that map our app CSS variable values to the
 * BlockNote Theme type, so the editor matches the active app theme.
 *
 * Colours match the values defined in globals.css for each theme.
 */

// ── Light ──────────────────────────────────────────────────────────────────
const light = {
  colors: {
    editor:   { text: '#0a0a0f', background: '#ffffff' },
    menu:     { text: '#0a0a0f', background: '#ffffff' },
    tooltip:  { text: '#0a0a0f', background: '#f3f4f6' },
    hovered:  { text: '#0a0a0f', background: '#f3f4f6' },
    selected: { text: '#ffffff', background: '#00d4aa' },
    disabled: { text: '#afafaf', background: '#f3f4f6' },
    shadow:   '#d1d5db',
    border:   '#e5e7eb',
    sideMenu: '#9ca3af',
  },
};

// ── Dark (warm charcoal) ──────────────────────────────────────────────────
const dark = {
  colors: {
    editor:   { text: '#e2e8f0', background: '#1e2028' },
    menu:     { text: '#e2e8f0', background: '#252830' },
    tooltip:  { text: '#e2e8f0', background: '#2d3040' },
    hovered:  { text: '#e2e8f0', background: '#2d3040' },
    selected: { text: '#1e2028', background: '#00d4aa' },
    disabled: { text: '#6b7280', background: '#252830' },
    shadow:   'transparent',
    border:   '#353840',
    sideMenu: '#6b7280',
  },
};

// ── Dracula ───────────────────────────────────────────────────────────────
const dracula = {
  colors: {
    editor:   { text: '#f8f8f2', background: '#282a36' },
    menu:     { text: '#f8f8f2', background: '#343746' },
    tooltip:  { text: '#f8f8f2', background: '#44475a' },
    hovered:  { text: '#f8f8f2', background: '#44475a' },
    selected: { text: '#282a36', background: '#bd93f9' },
    disabled: { text: '#6272a4', background: '#343746' },
    shadow:   'transparent',
    border:   '#44475a',
    sideMenu: '#6272a4',
  },
};

// ── Midnight ──────────────────────────────────────────────────────────────
const midnight = {
  colors: {
    editor:   { text: '#cbd5e1', background: '#0f1117' },
    menu:     { text: '#cbd5e1', background: '#161b27' },
    tooltip:  { text: '#cbd5e1', background: '#1e2535' },
    hovered:  { text: '#cbd5e1', background: '#1e2535' },
    selected: { text: '#0f1117', background: '#60a5fa' },
    disabled: { text: '#4b5563', background: '#161b27' },
    shadow:   'transparent',
    border:   '#1e2a3a',
    sideMenu: '#4b5563',
  },
};

export type AppThemeVariant = 'light' | 'dark' | 'dracula' | 'midnight';

/**
 * Returns a BlockNote { light, dark } theme pair where the dark side
 * matches the currently active app theme variant.
 */
export function getBlockNoteTheme(variant: AppThemeVariant) {
  const darkTheme = variant === 'dracula' ? dracula
    : variant === 'midnight' ? midnight
    : dark;

  return { light, dark: darkTheme };
}

