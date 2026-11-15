import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Bridge, listenForNative } from '../index.mjs';
import { connectMcp, connectNative } from '../native-client.mjs';

const capability = 'b'.repeat(64);
const binding = { version: 1, kind: 'clarification', threadId: 'thread-1', goalId: 'goal-1',
  goal: 'Complete the work', decision: 'Choose the direction?', expectedEventSha256: 'a'.repeat(64),
  senderId: '42', chatId: '44' };
const request = { id: 9, method: 'item/tool/requestUserInput', params: {
  threadId: 'thread-1', turnId: 'turn-1', itemId: 'item-1', isBlocking: true,
  autoResolutionMs: null, questions: [{ id: 'choice', question: binding.decision, options: [] }]
} };
const mcpRequest = { jsonrpc: '2.0', id: 'call-1', method: 'tools/call', params: {
  name: 'ask_owner', arguments: { question: binding.decision }, _meta: { threadId: binding.threadId }
} };

test('fixture client receives the exact JSON-RPC answer through its own socket', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-native-fixture-'));
  let questionResolve, answerResolve;
  const question = new Promise(resolve => { questionResolve = resolve; });
  const answer = new Promise(resolve => { answerResolve = resolve; });
  const store = { async load() { return null; }, async save() {} };
  let server, client;
  const bridge = new Bridge(binding, store, { async sendQuestion(text) { questionResolve(text); return '6'; } },
    { respond: (id, result) => server.respond(id, result) });
  try {
    await bridge.start();
    server = await listenForNative(dir, capability, bridge);
    await assert.rejects(connectNative({ socketPath: server.socketPath, capability: 'c'.repeat(64),
      binding, isCurrent: async () => true, sendResponse() {} }));
    client = await connectNative({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => true,
      sendResponse: (id, result) => answerResolve({ id, result }) });
    assert.equal(await client.forward({ ...request, method: 'item/fileChange/requestApproval' }), false);
    assert.equal(await client.forward(request), true);
    assert.equal(await question, binding.decision);
    await bridge.serial;
    const text = '  정확한 답\n☃  ';
    await bridge.onReply({ senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', text });
    assert.deepEqual(await answer, { id: 9, result: { answers: { choice: { answers: [text] } } } });
    assert.equal(bridge.state.nativeEffect, 'unknown');
    assert.equal(bridge.state.goalEffect, 'pending');
  } finally {
    client?.close();
    if (server) await server.close();
    await rm(dir, { recursive: true, force: true });
  }
});

test('a Goal change observed before native delivery leaves the old request unanswered', { timeout: 3000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-stale-fixture-'));
  let questionResolve, staleResolve;
  const question = new Promise(resolve => { questionResolve = resolve; });
  const stale = new Promise(resolve => { staleResolve = resolve; });
  const store = { async load() { return null; }, async save(value) {
    if (value.nativeEffect === 'not_sent') staleResolve();
  } };
  let server, client, checks = 0, responses = 0;
  const bridge = new Bridge(binding, store, { async sendQuestion(text) { questionResolve(text); return '6'; } },
    { respond: (id, result) => server.respond(id, result) });
  try {
    await bridge.start();
    server = await listenForNative(dir, capability, bridge);
    client = await connectNative({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => ++checks === 1, sendResponse: () => { responses += 1; } });
    assert.equal(await client.forward(request), true);
    await question; await bridge.serial;
    await bridge.onReply({ senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', text: 'answer' });
    await stale;
    assert.equal(responses, 0);
    assert.equal(bridge.state.phase, 'stale');
    assert.equal(bridge.state.goalEffect, 'pending');
  } finally {
    client?.close();
    if (server) await server.close();
    await rm(dir, { recursive: true, force: true });
  }
});

test('an already stale Goal does not fall back to another answer route', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-stale-before-fixture-'));
  let server, client, questions = 0, responses = 0, unavailable = 0;
  const bridge = new Bridge(binding, { async load() { return null; }, async save() {} },
    { async sendQuestion() { questions += 1; return '6'; } }, { async respond() { responses += 1; } });
  try {
    await bridge.start(); server = await listenForNative(dir, capability, bridge);
    client = await connectNative({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => false, sendResponse: () => { responses += 1; },
      onUnavailable: () => { unavailable += 1; } });
    assert.equal(await client.forward(request), true);
    assert.equal(questions, 0);
    assert.equal(responses, 0);
    assert.equal(unavailable, 1);
    assert.equal(bridge.state.phase, 'ready');
  } finally {
    client?.close();
    if (server) await server.close();
    await rm(dir, { recursive: true, force: true });
  }
});

test('the MCP connection returns the exact answer on the original request ID', { timeout: 3000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-fixture-'));
  let questionResolve, answerResolve;
  const question = new Promise(resolve => { questionResolve = resolve; });
  const answer = new Promise(resolve => { answerResolve = resolve; });
  let server, client;
  const bridge = new Bridge(binding, { async load() { return null; }, async save() {} },
    { async sendQuestion(text) { questionResolve(text); return '6'; } },
    { respond: (id, result) => server.respond(id, result) });
  try {
    await bridge.start(); server = await listenForNative(dir, capability, bridge);
    client = await connectMcp({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => true, sendResponse: (id, result) => answerResolve({ id, result }) });
    assert.equal(await client.forwardMcp(mcpRequest), true);
    assert.equal(await question, binding.decision); await bridge.serial;
    const text = '  원문\n☃  ';
    await bridge.onReply({ senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', text });
    assert.deepEqual(await answer, { id: 'call-1', result: { content: [{ type: 'text', text }] } });
    assert.equal(bridge.state.nativeEffect, 'unknown');
  } finally {
    client?.close(); if (server) await server.close(); await rm(dir, { recursive: true, force: true });
  }
});

test('MCP cancellation during the Goal recheck prevents the response', { timeout: 3000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-cancel-'));
  let questionResolve, checkingResolve, releaseCheck, staleResolve, checks = 0, responses = 0;
  const question = new Promise(resolve => { questionResolve = resolve; });
  const checking = new Promise(resolve => { checkingResolve = resolve; });
  const check = new Promise(resolve => { releaseCheck = resolve; });
  const stale = new Promise(resolve => { staleResolve = resolve; });
  let server, client;
  const store = { async load() { return null; }, async save(state) { if (state.phase === 'stale') staleResolve(); } };
  const bridge = new Bridge(binding, store, { async sendQuestion(text) { questionResolve(text); return '6'; } },
    { respond: (id, result) => server.respond(id, result) });
  try {
    await bridge.start(); server = await listenForNative(dir, capability, bridge);
    client = await connectMcp({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => { if (++checks === 1) return true; checkingResolve(); return check; },
      sendResponse: () => { responses += 1; } });
    await client.forwardMcp(mcpRequest); await question; await bridge.serial;
    await bridge.onReply({ senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', text: 'answer' });
    await checking;
    await client.cancelMcp({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 'call-1' } });
    releaseCheck(true); await stale;
    assert.equal(responses, 0);
    assert.equal(bridge.state.nativeEffect, 'unknown');
  } finally {
    client?.close(); if (server) await server.close(); await rm(dir, { recursive: true, force: true });
  }
});

test('MCP cancellation during the initial Goal check prevents a question', { timeout: 3000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-mcp-early-'));
  let checkingResolve, releaseCheck, questions = 0;
  const checking = new Promise(resolve => { checkingResolve = resolve; });
  const check = new Promise(resolve => { releaseCheck = resolve; });
  let server, client;
  const bridge = new Bridge(binding, { async load() { return null; }, async save() {} },
    { async sendQuestion() { questions += 1; return '6'; } }, { async respond() {} });
  try {
    await bridge.start(); server = await listenForNative(dir, capability, bridge);
    client = await connectMcp({ socketPath: server.socketPath, capability, binding,
      isCurrent: async () => { checkingResolve(); return check; }, sendResponse() {} });
    const forward = client.forwardMcp(mcpRequest); await checking;
    await client.cancelMcp({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 'call-1' } });
    releaseCheck(true); assert.equal(await forward, true);
    assert.equal(questions, 0);
  } finally {
    client?.close(); if (server) await server.close(); await rm(dir, { recursive: true, force: true });
  }
});
