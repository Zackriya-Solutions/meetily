/** Text to show for a failed command: Tauri commands reject with the backend's message as a string. */
export function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'string') return error;
  return error instanceof Error ? error.message : fallback;
}
