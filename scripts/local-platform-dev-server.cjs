const { spawn } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');
const SERVER_INFO_PATH = path.join(ROOT, '.tmp', 'platform-dev', 'server.json');
const DEFAULT_HOST = '127.0.0.1';
const DEFAULT_PORT = 14520;
const DEFAULT_TIMEOUT_MS = 30 * 60 * 1000;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function readServerInfo() {
  if (!fs.existsSync(SERVER_INFO_PATH)) {
    return null;
  }
  try {
    return JSON.parse(fs.readFileSync(SERVER_INFO_PATH, 'utf8'));
  } catch {
    return null;
  }
}

function requestOk(url) {
  return new Promise((resolve) => {
    const request = http.get(url, { timeout: 2000 }, (response) => {
      response.resume();
      response.on('end', () => {
        resolve(Boolean(response.statusCode && response.statusCode >= 200 && response.statusCode < 300));
      });
    });
    request.on('timeout', () => {
      request.destroy();
      resolve(false);
    });
    request.on('error', () => resolve(false));
  });
}

async function waitForPlatformDevServer(child, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (child.exitCode !== null) {
      throw new Error(`本地平台包服务提前退出: code=${child.exitCode}`);
    }

    const info = readServerInfo();
    const indexUrl = typeof info?.indexUrl === 'string' ? info.indexUrl : '';
    const reloadUrl = typeof info?.reloadUrl === 'string' ? info.reloadUrl : '';
    const healthBase = indexUrl.replace(/\/index\.local\.json$/u, '');
    if (indexUrl && reloadUrl && healthBase && await requestOk(`${healthBase}/health`)) {
      return { ...info, indexUrl, reloadUrl };
    }
    await sleep(500);
  }
  throw new Error('等待本地平台包服务启动超时');
}

function startLocalPlatformDevServer(options = {}) {
  fs.rmSync(SERVER_INFO_PATH, { force: true });
  const args = [
    'scripts/platform-dev-serve.cjs',
    '--host',
    options.host || DEFAULT_HOST,
    '--port',
    String(options.port || DEFAULT_PORT),
    '--build-adapters',
  ];
  if (Array.isArray(options.platforms) && options.platforms.length > 0) {
    args.push('--platform', options.platforms.join(','));
  }
  if (options.noBuildUi) {
    args.push('--no-build-ui');
  }

  const child = spawn(process.execPath, args, {
    cwd: ROOT,
    env: process.env,
    stdio: 'inherit',
    shell: false,
  });
  return child;
}

function terminateChild(child, signal = 'SIGTERM') {
  if (!child || child.exitCode !== null || child.killed) {
    return;
  }
  child.kill(signal);
}

module.exports = {
  startLocalPlatformDevServer,
  terminateChild,
  waitForPlatformDevServer,
};
