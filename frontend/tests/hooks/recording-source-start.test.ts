import { test } from 'bun:test';
import { fileURLToPath } from 'node:url';

// Existing suites replace entire context modules in Bun's shared module cache.
// Run the real recording hook in an isolated process so its context fixtures
// cannot depend on test order or change other suites' providers.
async function runFixture(name: string) {
  const fixture = fileURLToPath(new URL(`../fixtures/${name}.case.tsx`, import.meta.url));
  const child = Bun.spawn([process.execPath, 'test', fixture], { stdout: 'pipe', stderr: 'pipe' });
  const [status, stdout, stderr] = await Promise.all([
    child.exited, new Response(child.stdout).text(), new Response(child.stderr).text(),
  ]);
  if (status !== 0) throw new Error(`${stdout}\n${stderr}`);
}

test('manual, navigation, and sidebar starts preserve app-only source options', () => runFixture('recording-source-start'), 15000);
test('reload and stale events preserve the current backend source', () => runFixture('recording-source-status'), 15000);
