import { test } from 'bun:test';
import { fileURLToPath } from 'node:url';

async function runFixture(name: string, extension = 'tsx') {
  const fixture = fileURLToPath(new URL(`../fixtures/${name}.case.${extension}`, import.meta.url));
  const child = Bun.spawn([process.execPath, 'test', fixture], { stdout: 'pipe', stderr: 'pipe' });
  const [status, stdout, stderr] = await Promise.all([
    child.exited, new Response(child.stdout).text(), new Response(child.stderr).text(),
  ]);
  if (status !== 0) throw new Error(`${stdout}\n${stderr}`);
}

test('native recording completion refreshes once without resaving', () => runFixture('native-recording-completion'), 15000);
test('native session events synchronize controls and completion', () => runFixture('native-recording-state'), 15000);
test('native recording recovery remains idempotent and legacy recovery stays compatible', () => runFixture('native-recording-recovery'), 15000);

test('Stop controls handle pending, paused, duplicate, and failed stop requests', () => runFixture('recording-stop-controls'), 15000);
test('browser checkpoint saved state survives late and concurrent writes', () => runFixture('recording-checkpoint-persistence', 'ts'), 15000);
