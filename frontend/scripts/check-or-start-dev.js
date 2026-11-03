const { spawn, execSync } = require('child_process');
const http = require('http');
const path = require('path');

const PORT = 3118;
const URL = `http://localhost:${PORT}/`;

function checkIsReady() {
  return new Promise((resolve) => {
    const req = http.get(URL, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        if (res.statusCode === 200) {
          resolve(true);
        } else {
          resolve(false);
        }
      });
    });
    req.on('error', () => resolve(false));
    req.setTimeout(2000, () => {
      req.destroy();
      resolve(false);
    });
  });
}

async function main() {
  const isAlreadyRunning = await checkIsReady();
  if (isAlreadyRunning) {
    console.log(`✅ Next.js dev server is already running and ready on port ${PORT}.`);
    // Keep process alive so Tauri CLI doesn't terminate beforeDevCommand
    setInterval(() => {}, 1000 * 60 * 60);
    return;
  }

  console.log(`🚀 Starting Next.js dev server on port ${PORT}...`);
  const isWin = process.platform === 'win32';
  const nextCmd = isWin ? 'npx.cmd' : 'npx';
  const rootDir = path.join(__dirname, '..');

  const child = spawn(nextCmd, ['next', 'dev', '-p', String(PORT)], {
    cwd: rootDir,
    stdio: 'inherit',
    shell: true
  });

  // Pre-warm / compile root route
  const start = Date.now();
  let warmed = false;
  while (!warmed && Date.now() - start < 60000) {
    await new Promise(r => setTimeout(r, 800));
    warmed = await checkIsReady();
    if (warmed) {
      console.log(`\n✨ Meetily frontend compiled and ready on port ${PORT}!\n`);
      break;
    }
  }

  function cleanup() {
    if (child && !child.killed) {
      if (isWin) {
        try {
          execSync(`taskkill /pid ${child.pid} /T /F`, { stdio: 'ignore' });
        } catch (e) {}
      } else {
        child.kill('SIGTERM');
      }
    }
    process.exit(0);
  }

  process.on('SIGINT', cleanup);
  process.on('SIGTERM', cleanup);
  process.on('exit', cleanup);
}

main().catch(err => {
  console.error('Error in check-or-start-dev:', err);
  process.exit(1);
});
