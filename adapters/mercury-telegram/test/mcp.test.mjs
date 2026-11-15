import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { Bridge, listenForNative } from '../index.mjs';
import { createMcpServer, loadMcpSettings, projectGoalIsCurrent } from '../mcp.mjs';

const binding = { version: 1, kind: 'clarification', threadId: 'thread-1', goalId: 'goal-1',
  goal: 'Complete the work', decision: 'Choose the direction?', expectedEventSha256: 'a'.repeat(64),
  senderId: '42', chatId: '44', capability: 'b'.repeat(64) };
const openGoal = () => ({ goalId: binding.goalId, goal: binding.goal, eventSha256: binding.expectedEventSha256,
  closure: { closed: false }, decisions: [{ id: binding.decision, text: binding.decision, disposition: 'open' }] });
const call = (id, threadId = binding.threadId, question = binding.decision) => ({ jsonrpc: '2.0', id,
  method: 'tools/call', params: { name: 'ask_owner', arguments: { question }, _meta: { threadId } } });
const cancel = id => ({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: id } });
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(condition) {
  for (let i = 0; i < 200; i += 1) { if (condition()) return; await delay(5); }
  throw new Error('fixture outcome did not arrive');
}

test('Goal check uses only a project status read and verifies the exact open decision', async () => {
  const settings = { exitbindCommand: '/fixture/exitbind', projectConfig: '/fixture/control.json' };
  const calls = [];
  const run = async (command, args, options) => { calls.push({ command, args, options }); return { stdout: JSON.stringify(openGoal()) }; };
  assert.equal(await projectGoalIsCurrent(binding, settings, run), true);
  assert.deepEqual(calls[0].args, ['goal', 'status', '--project', '--json', '--expected-sha256',
    binding.expectedEventSha256, '--config', settings.projectConfig]);
  assert.equal(calls[0].command, settings.exitbindCommand);
  for (const name of ['CODEX_THREAD_ID', 'CODEX_SESSION_ID', 'CLAUDE_CODE_SESSION_ID', 'EXITBIND_NATIVE_SESSION_ID']) {
    assert.equal(Object.hasOwn(calls[0].options.env, name), false);
  }
  for (const change of [
    { goalId: 'other' }, { goal: 'other' }, { eventSha256: 'c'.repeat(64) },
    { closure: { closed: true } }, { decisions: [] },
    { decisions: [{ id: binding.decision, text: binding.decision, disposition: 'accepted' }] }
  ]) {
    assert.equal(await projectGoalIsCurrent(binding, settings, async () => ({ stdout: JSON.stringify({ ...openGoal(), ...change }) })), false);
  }
  assert.equal(await projectGoalIsCurrent(binding, settings, async () => { throw new Error('unavailable'); }), false);
});

test('configuration and binding must be private files', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-settings-'));
  try {
    const bindingFile = join(dir, 'binding.json'); const settingsFile = join(dir, 'settings.json');
    await writeFile(bindingFile, JSON.stringify(binding), { mode: 0o600 });
    await writeFile(settingsFile, JSON.stringify({ version: 1, bindingFile, socketPath: join(dir, 'bridge.sock'),
      exitbindCommand: '/fixture/exitbind', projectConfig: '/fixture/control.json' }), { mode: 0o600 });
    assert.equal((await loadMcpSettings(settingsFile)).binding.decision, binding.decision);
    await chmod(bindingFile, 0o644);
    await assert.rejects(loadMcpSettings(settingsFile));
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('foreign, mismatched and duplicate calls never attach or resolve the original request', async () => {
  const sent = []; let connections = 0;
  const server = createMcpServer({ binding, socketPath: '/fixture/socket', isCurrent: async () => true,
    send: value => sent.push(value), connect: async () => { connections += 1; throw new Error('must not attach'); } });
  server.handle(call(1, 'other-thread'));
  server.handle(call(2, binding.threadId, 'different question'));
  assert.equal(connections, 0);
  assert.deepEqual(sent.map(x => x.error.code), [-32602, -32602]);
  let release;
  const waiting = new Promise(resolve => { release = resolve; });
  const pending = createMcpServer({ binding, socketPath: '/fixture/socket', isCurrent: () => waiting,
    send: value => sent.push(value), connect: async () => { connections += 1; throw new Error('must not attach'); } });
  pending.handle(call(3));
  pending.handle({ ...call(3), params: { name: 'report_observation', arguments: { stage: 'native' }, _meta: { threadId: binding.threadId } } });
  assert.equal(sent.some(x => x.id === 3), false);
  pending.handle(cancel(3)); release(true); await delay(10);
  assert.equal(connections, 0);
  assert.equal(sent.some(x => x.id === 3), false);
  server.close(); pending.close();
});

test('cancellation while the bridge is connecting never forwards a question', async () => {
  const sent = []; let release, connecting = false, closed = false, forwarded = false;
  const connection = new Promise(resolve => { release = resolve; });
  const server = createMcpServer({ binding, socketPath: '/fixture/socket', isCurrent: async () => true,
    send: value => sent.push(value), connect: () => { connecting = true; return connection; } });
  server.handle(call(4));
  await until(() => connecting);
  server.handle(cancel(4));
  release({ close() { closed = true; }, async forwardMcp() { forwarded = true; } });
  await until(() => closed);
  assert.equal(forwarded, false);
  assert.equal(sent.some(x => x.id === 4), false);
  server.close();
});

async function fixture() {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-bridge-'));
  const output = []; let question;
  const delivered = new Promise(resolve => { question = resolve; });
  let socket;
  const bridge = new Bridge(binding, { async load() { return null; }, async save() {} },
    { async sendQuestion(text) { question(text); return '6'; } },
    { respond: (id, result) => socket.respond(id, result) });
  await bridge.start(); socket = await listenForNative(dir, binding.capability, bridge);
  let currentGoal = openGoal();
  const settings = { exitbindCommand: '/fixture/exitbind', projectConfig: '/fixture/control.json' };
  const server = createMcpServer({ binding, socketPath: socket.socketPath,
    isCurrent: value => projectGoalIsCurrent(value, settings, async () => ({ stdout: JSON.stringify(currentGoal) })),
    send: value => output.push(value) });
  return { dir, output, bridge, server, delivered, setGoalEvent: eventSha256 => { currentGoal = { ...currentGoal, eventSha256 }; },
    async close() { server.close(); await socket.close(); await rm(dir, { recursive: true, force: true }); } };
}

test('actual local bridge returns exact answer and records separate reported effects', async () => {
  const f = await fixture();
  try {
    f.server.handle(call(7));
    assert.equal(await f.delivered, binding.decision);
    await f.bridge.serial;
    const answer = '  정확한 답\n☃  ';
    await f.bridge.onReply({ senderId: '42', chatId: '44', messageId: '8', replyToMessageId: '6', text: answer });
    await until(() => f.output.some(x => x.id === 7));
    assert.deepEqual(f.output.find(x => x.id === 7).result.content, [{ type: 'text', text: answer }]);
    f.server.handle({ jsonrpc: '2.0', id: 8, method: 'tools/call', params: { name: 'report_observation',
      _meta: { threadId: binding.threadId }, arguments: { stage: 'native', evidenceRef: 'fixture-native-result' } } });
    await until(() => f.bridge.state.nativeEffect === 'accepted_reported');
    const record = { goalId: binding.goalId, goal: binding.goal, previousEventSha256: binding.expectedEventSha256,
      eventSha256: 'c'.repeat(64), decisions: [{ id: binding.decision, text: binding.decision,
        disposition: 'accepted', resultRefs: ['fixture-goal-result'] }] };
    f.server.handle({ jsonrpc: '2.0', id: 9, method: 'tools/call', params: { name: 'report_observation',
      _meta: { threadId: binding.threadId }, arguments: { stage: 'goal', record, resultRef: 'fixture-goal-result' } } });
    await until(() => f.bridge.state.phase === 'reported_complete');
    assert.equal(Object.hasOwn(f.bridge.state, 'answer'), false);
    assert.equal(f.bridge.state.goalEffect, 'recorded_reported');
  } finally { await f.close(); }
});

test('an unanswered question stays pending; cancellation prevents a later reply', async () => {
  const f = await fixture();
  try {
    f.server.handle(call(10)); await f.delivered; await f.bridge.serial;
    await delay(30);
    assert.equal(f.output.some(x => x.id === 10), false);
    assert.equal(f.bridge.state.phase, 'waiting');
    f.server.handle(cancel(10));
    await until(() => f.bridge.state.phase === 'request_cleared');
    assert.equal(await f.bridge.onReply({ senderId: '42', chatId: '44', messageId: '8',
      replyToMessageId: '6', text: 'late' }), false);
    assert.equal(f.output.some(x => x.id === 10), false);
  } finally { await f.close(); }
});

test('Goal changes before an answer prevent its native delivery', async () => {
  const f = await fixture();
  try {
    f.server.handle(call(11)); await f.delivered; await f.bridge.serial;
    f.setGoalEvent('d'.repeat(64));
    await f.bridge.onReply({ senderId: '42', chatId: '44', messageId: '8', replyToMessageId: '6', text: 'answer' });
    await until(() => f.output.some(x => x.id === 11));
    await until(() => f.bridge.state.phase === 'stale');
    assert.equal(f.output.find(x => x.id === 11).error.code, -32000);
    assert.equal(f.bridge.state.nativeEffect, 'not_sent');
  } finally { await f.close(); }
});

test('stdio process speaks MCP and only invokes the project Goal reader', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-stdio-'));
  let socket, child;
  try {
    const bindingFile = join(dir, 'binding.json'); const settingsFile = join(dir, 'settings.json');
    const reader = join(dir, 'reader.cjs'); const calls = join(dir, 'reader-calls.jsonl');
    await writeFile(bindingFile, JSON.stringify(binding), { mode: 0o600 });
    const script = `#!/usr/bin/env node\nconst fs = require('node:fs');\n` +
      `fs.appendFileSync(${JSON.stringify(calls)}, JSON.stringify({args:process.argv.slice(2), ` +
      `thread:Object.hasOwn(process.env,'CODEX_THREAD_ID')}) + '\\n');\n` +
      `process.stdout.write(${JSON.stringify(JSON.stringify(openGoal()))});\n`;
    await writeFile(reader, script, { mode: 0o700 });
    let question;
    const delivered = new Promise(resolve => { question = resolve; });
    const bridge = new Bridge(binding, { async load() { return null; }, async save() {} },
      { async sendQuestion(text) { question(text); return '6'; } },
      { respond: (id, result) => socket.respond(id, result) });
    await bridge.start(); socket = await listenForNative(dir, binding.capability, bridge);
    await writeFile(settingsFile, JSON.stringify({ version: 1, bindingFile, socketPath: socket.socketPath,
      exitbindCommand: reader, projectConfig: join(dir, 'fixture-control.json') }), { mode: 0o600 });
    child = spawn(process.execPath, [fileURLToPath(new URL('../mcp.mjs', import.meta.url)), settingsFile],
      { stdio: ['pipe', 'pipe', 'pipe'] });
    const messages = [];
    createInterface({ input: child.stdout }).on('line', line => messages.push(JSON.parse(line)));
    const send = message => child.stdin.write(`${JSON.stringify(message)}\n`);
    send({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-03-26' } });
    await until(() => messages.some(x => x.id === 1));
    assert.equal(messages.find(x => x.id === 1).result.protocolVersion, '2025-03-26');
    send(call(2));
    assert.equal(await delivered, binding.decision); await bridge.serial;
    await bridge.onReply({ senderId: '42', chatId: '44', messageId: '8', replyToMessageId: '6', text: '  exact\nanswer  ' });
    await until(() => messages.some(x => x.id === 2));
    assert.equal(messages.find(x => x.id === 2).result.content[0].text, '  exact\nanswer  ');
    const invocations = (await readFile(calls, 'utf8')).trim().split('\n').map(JSON.parse);
    assert.ok(invocations.length >= 2);
    assert.ok(invocations.every(x => x.args[0] === 'goal' && x.args[1] === 'status' &&
      x.args.includes('--project') && !x.thread));
  } finally {
    if (child) {
      child.stdin.end();
      if (child.exitCode === null) {
        await Promise.race([new Promise(resolve => child.once('exit', resolve)), delay(1000)]);
        if (child.exitCode === null) child.kill();
      }
    }
    if (socket) await socket.close();
    await rm(dir, { recursive: true, force: true });
  }
});
