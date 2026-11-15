// A single owner-bound MCP question for an already loaded Codex thread.
// Goal writes and native-observation decisions remain with the owning Lead.
import { constants } from 'node:fs';
import { open } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { connectMcp } from './native-client.mjs';
import { currentMcpRequest } from './index.mjs';

const execFileAsync = promisify(execFile);
const HASH = /^[0-9a-f]{64}$/;
const ID = /^[A-Za-z0-9_.:-]{1,256}$/;
const MAX_FRAME = 64 * 1024;

async function readPrivateJson(path) {
  if (typeof path !== 'string' || !isAbsolute(path) || typeof process.getuid !== 'function') {
    throw new Error('private configuration unavailable');
  }
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o077) !== 0 || stat.size > MAX_FRAME) {
      throw new Error('private configuration unavailable');
    }
    return JSON.parse(await file.readFile('utf8'));
  } finally { await file.close(); }
}

function validateBinding(binding) {
  if (!binding || binding.version !== 1 || binding.kind !== 'clarification' ||
      ![binding.threadId, binding.goalId, binding.senderId, binding.chatId].every(x => typeof x === 'string' && ID.test(x)) ||
      typeof binding.goal !== 'string' || !binding.goal.trim() ||
      typeof binding.decision !== 'string' || !binding.decision.trim() ||
      binding.decision !== binding.decision.trim() || binding.decision.length > 3900 ||
      !HASH.test(binding.expectedEventSha256) || !HASH.test(binding.capability)) {
    throw new Error('private binding unavailable');
  }
  return binding;
}

export async function loadMcpSettings(path) {
  const settings = await readPrivateJson(path);
  if (!settings || settings.version !== 1 ||
      ![settings.bindingFile, settings.socketPath, settings.exitbindCommand, settings.projectConfig]
        .every(x => typeof x === 'string' && isAbsolute(x))) {
    throw new Error('private configuration unavailable');
  }
  return { settings, binding: validateBinding(await readPrivateJson(settings.bindingFile)) };
}

// The project selector is explicit. Removing inherited session selectors is
// defensive; this process neither selects nor writes another session's Goal.
export async function projectGoalIsCurrent(binding, settings, run = execFileAsync) {
  try {
    const env = { ...process.env };
    for (const key of ['CODEX_THREAD_ID', 'CODEX_SESSION_ID', 'CLAUDE_CODE_SESSION_ID', 'EXITBIND_NATIVE_SESSION_ID']) delete env[key];
    const { stdout } = await run(settings.exitbindCommand,
      ['goal', 'status', '--project', '--json', '--expected-sha256', binding.expectedEventSha256,
        '--config', settings.projectConfig],
      { env, encoding: 'utf8', timeout: 10000, maxBuffer: 1024 * 1024 });
    const goal = JSON.parse(stdout);
    return goal.goalId === binding.goalId && goal.goal === binding.goal &&
      goal.eventSha256 === binding.expectedEventSha256 && goal.closure?.closed === false &&
      Array.isArray(goal.decisions) && goal.decisions.some(item => item?.id === binding.decision &&
        item.text === binding.decision && item.disposition === 'open');
  } catch { return false; }
}

function validRequestId(id) {
  return (typeof id === 'string' && id.length > 0 && id.length <= 256) ||
    (typeof id === 'number' && Number.isSafeInteger(id));
}

function validObservationRef(value) {
  return typeof value === 'string' && value.trim().length > 0 && value.length <= 4096;
}

function validGoalRecord(record, binding, ref) {
  return record && record.goalId === binding.goalId && record.goal === binding.goal &&
    record.previousEventSha256 === binding.expectedEventSha256 && HASH.test(record.eventSha256) &&
    Array.isArray(record.decisions) && record.decisions.some(item => item?.id === binding.decision &&
      item.text === binding.decision && item.disposition === 'accepted' &&
      Array.isArray(item.resultRefs) && item.resultRefs.includes(ref));
}

// send must begin the stdout write synchronously. In particular, it must not
// wait after the final Goal/pending check, when a cancellation could arrive.
export function createMcpServer({ binding, socketPath, isCurrent, send, connect = connectMcp }) {
  const pending = new Map();
  const seen = new Set();
  const reports = new Set();
  let client = null;
  let used = false;
  let answered = null;
  let closed = false;
  const keyOf = id => JSON.stringify(id);
  const reply = (id, result) => send({ jsonrpc: '2.0', id, result });
  const error = (id, code, message) => send({ jsonrpc: '2.0', id, error: { code, message } });

  async function ask(message) {
    const id = message.id;
    const key = keyOf(id);
    if (!currentMcpRequest(message, binding) || !message.params.arguments ||
        Object.keys(message.params.arguments).some(k => k !== 'question')) {
      error(id, -32602, 'Owner question does not match the active binding.');
      return;
    }
    if (used) { error(id, -32000, 'This owner question binding was already used.'); return; }
    used = true;
    const request = { cancelled: false, forwardStarted: false };
    pending.set(key, request);
    try {
      if (await isCurrent(binding) !== true) {
        if (pending.delete(key)) error(id, -32000, 'The bound Goal decision is unavailable or no longer current.');
        return;
      }
      if (closed || request.cancelled || !pending.has(key)) return;
      client = await connect({ socketPath, capability: binding.capability, binding, isCurrent,
        sendResponse(responseId, result) {
          const responseKey = keyOf(responseId);
          if (closed || !pending.has(responseKey)) return;
          if (!result || !Array.isArray(result.content) || result.content.length !== 1 ||
              result.content[0]?.type !== 'text' || typeof result.content[0].text !== 'string' ||
              result.content[0].text.length === 0) throw new Error('invalid owner response');
          pending.delete(responseKey);
          answered = { id: responseId, answerSha256: createHash('sha256').update(result.content[0].text).digest('hex') };
          reply(responseId, result);
        },
        onUnavailable() {
          if (!closed && pending.delete(key)) error(id, -32000, 'Owner bridge is unavailable; no answer was delivered.');
        }
      });
      if (closed || request.cancelled || !pending.has(key)) { client.close(); return; }
      request.forwardStarted = true;
      if (await client.forwardMcp(message) !== true && pending.delete(key)) {
        error(id, -32602, 'Owner question does not match the active binding.');
      }
    } catch {
      if (!closed && pending.delete(key)) error(id, -32000, 'Owner bridge is unavailable; no answer was delivered.');
    }
  }

  async function report(message) {
    const args = message.params?.arguments;
    const id = message.id;
    if (message.params?._meta?.threadId !== binding.threadId || !answered || !client ||
        !args || typeof args !== 'object' || Array.isArray(args) || !['native', 'goal'].includes(args.stage)) {
      error(id, -32602, 'Observation does not match this completed request.'); return;
    }
    if (reports.has(args.stage)) { error(id, -32000, 'This observation was already forwarded.'); return; }
    if (args.stage === 'native' && (Object.keys(args).some(k => !['stage', 'evidenceRef'].includes(k)) ||
        !validObservationRef(args.evidenceRef))) {
      error(id, -32602, 'A native observation reference is required.'); return;
    }
    if (args.stage === 'goal' && (Object.keys(args).some(k => !['stage', 'record', 'resultRef'].includes(k)) ||
        !validObservationRef(args.resultRef) || !validGoalRecord(args.record, binding, args.resultRef))) {
      error(id, -32602, 'A matching guarded Goal record is required.'); return;
    }
    // Mark before the write: an uncertain write is not permission to replay.
    reports.add(args.stage);
    try {
      if (args.stage === 'native') await client.reportNative({ threadId: binding.threadId,
        requestId: answered.id, outcome: 'accepted', answerSha256: answered.answerSha256, evidenceRef: args.evidenceRef });
      else await client.reportGoal({ record: args.record, resultRef: args.resultRef });
      reply(id, { content: [{ type: 'text', text: 'Observation forwarded; confirm the bridge state separately.' }] });
    } catch { error(id, -32000, 'Observation write is uncertain; inspect before recovery.'); }
  }

  return {
    handle(message) {
      if (closed || !message || message.jsonrpc !== '2.0' || typeof message.method !== 'string') return;
      if (message.method === 'notifications/cancelled') {
        const key = keyOf(message.params?.requestId);
        const request = pending.get(key);
        if (!request) return;
        request.cancelled = true;
        pending.delete(key);
        if (client && request.forwardStarted) void client.cancelMcp(message);
        return;
      }
      if (message.method.startsWith('notifications/')) return;
      if (!validRequestId(message.id)) return;
      // An error for a reused ID could resolve the original in-flight call.
      if (seen.has(keyOf(message.id))) return;
      seen.add(keyOf(message.id));
      if (message.method === 'initialize') {
        reply(message.id, { protocolVersion: message.params?.protocolVersion || '2025-03-26',
          capabilities: { tools: {} }, serverInfo: { name: 'exitbind-owner-bridge', version: '0.1.0' } });
      } else if (message.method === 'ping') reply(message.id, {});
      else if (message.method === 'tools/list') reply(message.id, { tools: [
        { name: 'ask_owner', description: 'Wait for the bound owner to answer the current clarification.',
          inputSchema: { type: 'object', properties: { question: { type: 'string' } }, required: ['question'], additionalProperties: false } },
        { name: 'report_observation', description: 'Forward a separately observed native or guarded Goal result for this question.',
          inputSchema: { type: 'object', properties: { stage: { type: 'string', enum: ['native', 'goal'] },
            evidenceRef: { type: 'string' }, resultRef: { type: 'string' }, record: { type: 'object' } }, required: ['stage'] } }
      ] });
      else if (message.method === 'tools/call' && message.params?.name === 'ask_owner') void ask(message);
      else if (message.method === 'tools/call' && message.params?.name === 'report_observation') void report(message);
      else error(message.id, -32601, 'Method or tool not found.');
    },
    close() { closed = true; pending.clear(); client?.close(); }
  };
}

async function main() {
  if (process.argv.length !== 3) throw new Error('private configuration required');
  const { settings, binding } = await loadMcpSettings(process.argv[2]);
  const server = createMcpServer({ binding, socketPath: settings.socketPath,
    isCurrent: value => projectGoalIsCurrent(value, settings),
    send(value) {
      if (process.stdout.destroyed) throw new Error('MCP output unavailable');
      process.stdout.write(`${JSON.stringify(value)}\n`);
    }
  });
  let buffer = Buffer.alloc(0);
  process.stdin.on('data', chunk => {
    buffer = Buffer.concat([buffer, chunk]);
    let end;
    while ((end = buffer.indexOf(10)) >= 0) {
      if (end > MAX_FRAME) { server.close(); process.stdin.destroy(); process.exitCode = 1; return; }
      const line = buffer.subarray(0, end); buffer = buffer.subarray(end + 1);
      try { server.handle(JSON.parse(line.toString('utf8'))); }
      catch { /* malformed frames are not owner answers */ }
    }
    if (buffer.length > MAX_FRAME) { server.close(); process.stdin.destroy(); process.exitCode = 1; }
  });
  process.stdin.on('end', () => server.close());
  process.stdin.on('error', () => server.close());
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch(() => { process.stderr.write('Owner bridge configuration unavailable.\n'); process.exitCode = 1; });
}
