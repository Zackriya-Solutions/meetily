import { test } from 'bun:test';
import { fileURLToPath } from 'node:url';

test('summary editor defers loading while preserving Save and Copy', async () => {
  const fixture = fileURLToPath(new URL('../fixtures/summary-editor-loading.case.tsx', import.meta.url));
  const child = Bun.spawn([process.execPath, 'test', fixture], { stdout: 'pipe', stderr: 'pipe' });
  const [status, stdout, stderr] = await Promise.all([
    child.exited, new Response(child.stdout).text(), new Response(child.stderr).text(),
  ]);
  if (status !== 0) throw new Error(`${stdout}\n${stderr}`);
}, 15000);
