#!/usr/bin/env node
/**
 * kill-port.js <port>
 *
 * Kills any process listening on the given TCP port.
 * Cross-platform: macOS/Linux use lsof, Windows uses netstat+taskkill.
 * Exits 0 whether or not anything was running.
 */

const { spawnSync } = require('child_process');
const os = require('os');

const port = parseInt(process.argv[2], 10);
if (!port) {
  console.error('Usage: node kill-port.js <port>');
  process.exit(1);
}

const platform = os.platform();

if (platform === 'win32') {
  // Windows: find PIDs via netstat then taskkill each one
  const netstat = spawnSync('netstat', ['-ano'], { encoding: 'utf8' });
  const lines = (netstat.stdout || '').split('\n');
  const pids = new Set();
  for (const line of lines) {
    if (line.includes(`:${port} `) && line.includes('LISTENING')) {
      const parts = line.trim().split(/\s+/);
      const pid = parts[parts.length - 1];
      if (pid && /^\d+$/.test(pid) && pid !== '0') pids.add(pid);
    }
  }
  for (const pid of pids) {
    const r = spawnSync('taskkill', ['/F', '/PID', pid], { encoding: 'utf8' });
    if (r.status === 0) {
      console.log(`Killed PID ${pid} on port ${port}`);
    }
  }
} else {
  // macOS / Linux: lsof gives all PIDs bound to the port
  const lsof = spawnSync('lsof', ['-ti', `tcp:${port}`], { encoding: 'utf8' });
  const pids = (lsof.stdout || '').trim().split('\n').filter(Boolean);
  for (const pid of pids) {
    try {
      process.kill(parseInt(pid, 10), 'SIGKILL');
      console.log(`Killed PID ${pid} on port ${port}`);
    } catch (err) {
      // Already gone — ignore
    }
  }
}

console.log(`Port ${port} is clear.`);

