// Optional integration for an existing Codex app-server client. The client owns
// its native connection and passes the response back on that same connection.
import { createConnection } from 'node:net';
import { lstat } from 'node:fs/promises';
import { dirname, isAbsolute } from 'node:path';
import { currentMcpRequest, currentRequest } from './index.mjs';

const HASH = /^[0-9a-f]{64}$/;
const MAX_FRAME = 64 * 1024;

export async function connectNative(options) { return connect({ ...options, transport: 'native' }); }

// A separate MCP adapter retains the original tools/call ID and cancellation
// lifetime; it never pretends an MCP call is a Codex server request.
export async function connectMcp(options) { return connect({ ...options, transport: 'mcp' }); }

async function connect({ socketPath, capability, binding, isCurrent, sendResponse, onUnavailable = () => {}, transport }) {
  if (!isAbsolute(socketPath) || typeof capability !== 'string' || !HASH.test(capability) ||
      typeof sendResponse !== 'function' || typeof isCurrent !== 'function') throw new Error('invalid native bridge connection');
  const [dir, socketFile] = await Promise.all([lstat(dirname(socketPath)), lstat(socketPath)]);
  if (!dir.isDirectory() || dir.uid !== process.getuid() || (dir.mode & 0o077) !== 0 ||
      !socketFile.isSocket() || socketFile.uid !== process.getuid() || (socketFile.mode & 0o077) !== 0) {
    throw new Error('native bridge socket is unavailable or unsafe');
  }
  const socket = createConnection(socketPath);
  let buffer = '';
  let attached = false;
  const pending = new Set();
  const known = new Set();
  let readyResolve, readyReject;
  const ready = new Promise((resolve, reject) => { readyResolve = resolve; readyReject = reject; });
  const write = value => new Promise((resolve, reject) => {
    if (!socket.writable) return reject(new Error('native bridge disconnected'));
    socket.write(`${JSON.stringify(value)}\n`, e => e ? reject(e) : resolve());
  });
  socket.setEncoding('utf8');
  socket.on('connect', () => { void write({ type: 'attach', threadId: binding.threadId, capability }).catch(readyReject); });
  socket.on('error', () => {
    if (transport === 'mcp') pending.clear();
    if (!attached) readyReject(new Error('native bridge unavailable'));
    else onUnavailable('native bridge disconnected; request outcome may be unknown');
  });
  socket.on('close', () => {
    if (transport === 'mcp') pending.clear();
    if (!attached) readyReject(new Error('native bridge refused connection'));
    else onUnavailable('native bridge disconnected; request outcome may be unknown');
  });
  socket.on('data', chunk => {
    buffer += chunk;
    if (Buffer.byteLength(buffer) > MAX_FRAME) { socket.destroy(); return; }
    let newline;
    while ((newline = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
      let m;
      try { m = JSON.parse(line); } catch { socket.destroy(); return; }
      if (!attached && m.type === 'attached') { attached = true; readyResolve(); continue; }
      if (!attached) { socket.destroy(); return; }
      if (m.type === 'response') {
        const key = JSON.stringify(m.id);
        if (transport === 'mcp') {
          if (!pending.has(key)) continue;
          Promise.resolve().then(async () => {
            let current = false;
            try { current = await isCurrent(binding) === true; } catch { /* unavailable is not current */ }
            // Cancellation and disconnect remove this ID while isCurrent waits.
            if (!pending.has(key) || socket.destroyed) return;
            pending.delete(key);
            if (!current) {
              onUnavailable('bound Goal is no longer current; MCP response was not sent');
              await write({ type: 'responseNotSent', id: m.id });
              return;
            }
            await sendResponse(m.id, m.result);
          }).catch(() => onUnavailable('MCP response outcome is unknown; do not replay'));
          continue;
        }
        if (!pending.delete(key)) continue;
        Promise.resolve().then(async () => {
          let current = false;
          try { current = await isCurrent(binding) === true; } catch { /* unavailable is not current */ }
          if (!current) {
            onUnavailable('bound Goal is no longer current; native response was not sent');
            await write({ type: 'responseNotSent', id: m.id });
            return;
          }
          await sendResponse(m.id, m.result);
        }).catch(() => onUnavailable('native response outcome is unknown; do not replay'));
      } else if (m.type === 'requestUncertain' || (m.type === 'requestObserved' && !m.accepted)) {
        onUnavailable('bound native request was not delivered; inspect before continuing');
      }
    }
  });
  await ready;
  if (transport === 'mcp') {
    return {
      async forwardMcp(message) {
        if (!currentMcpRequest(message, binding)) return false;
        const key = JSON.stringify(message.id);
        if (known.has(key)) return true;
        known.add(key);
        pending.add(key);
        let current = false;
        try { current = await isCurrent(binding) === true; } catch { /* unavailable is not current */ }
        if (!pending.has(key) || socket.destroyed) return true;
        if (!current) {
          pending.delete(key);
          onUnavailable('bound Goal is not current; the MCP request remains unanswered');
          return true;
        }
        try { await write({ type: 'mcpRequest', message }); }
        catch { onUnavailable('MCP bridge write outcome is unknown; do not replay'); }
        return true;
      },
      // Delete before returning the promise, including during an in-flight Goal check.
      cancelMcp(message) {
        if (message?.jsonrpc !== '2.0' || message.method !== 'notifications/cancelled') return Promise.resolve(false);
        const key = JSON.stringify(message.params?.requestId);
        if (!known.has(key)) return Promise.resolve(false);
        pending.delete(key);
        return write({ type: 'mcpCancelled', message }).then(() => true).catch(() => {
          onUnavailable('MCP cancellation outcome is unknown'); return false;
        });
      },
      async reportNative(observation) { await write({ type: 'nativeObservation', observation }); },
      async reportGoal(observation) { await write({ type: 'goalObservation', observation }); },
      close() { pending.clear(); socket.destroy(); }
    };
  }
  return {
    // Returns true only for the bound clarification. A true result leaves the
    // request pending even if transport subsequently fails; no fallback answer.
    async forward(message) {
      if (!currentRequest(message, binding)) return false;
      let current = false;
      try { current = await isCurrent(binding) === true; } catch { /* unavailable is not current */ }
      if (!current) {
        onUnavailable('bound Goal is not current; the native request remains unanswered');
        return true;
      }
      const key = JSON.stringify(message.id);
      if (pending.has(key)) return true;
      pending.add(key);
      try { await write({ type: 'request', message }); }
      catch { onUnavailable('native bridge write outcome is unknown; do not replay'); }
      return true;
    },
    async resolved(message) { await write({ type: 'resolved', message }); },
    async reportNative(observation) { await write({ type: 'nativeObservation', observation }); },
    async reportGoal(observation) { await write({ type: 'goalObservation', observation }); },
    close() { socket.destroy(); }
  };
}
