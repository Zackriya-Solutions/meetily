/**
 * copyToClipboard — uses the Tauri clipboard-manager plugin when running
 * inside Tauri, falls back to execCommand('copy') otherwise.
 *
 * The native Clipboard API (navigator.clipboard.writeText) is deliberately
 * avoided because it is blocked in Tauri's webview without a secure context
 * or when the window doesn't have focus.
 */
export async function copyToClipboard(text: string): Promise<void> {
  // 1. Tauri clipboard plugin (preferred — works reliably in the webview)
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    try {
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
      await writeText(text);
      return;
    } catch {
      // fall through
    }
  }

  // 2. Modern Clipboard API (works in browsers / dev server preview)
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // fall through
    }
  }

  // 3. Legacy execCommand fallback
  const el = document.createElement('textarea');
  el.value = text;
  el.setAttribute('readonly', '');
  el.style.cssText = 'position:fixed;top:-9999px;left:-9999px;opacity:0;';
  document.body.appendChild(el);
  el.select();
  el.setSelectionRange(0, el.value.length);
  document.execCommand('copy');
  document.body.removeChild(el);
}
