import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { Bridge } from '../index.mjs';

const binding = { version: 1, kind: 'clarification', threadId: 'thread-1', goalId: 'goal-1',
  goal: 'Complete the work', decision: 'Choose the direction?', expectedEventSha256: 'a'.repeat(64),
  senderId: '42', chatId: '44', codex: '/bin/codex', socket: '/tmp/codex.sock' };
const request = { id: 3, method: 'item/tool/requestUserInput', params: {
  threadId: 'thread-1', turnId: 'turn-1', itemId: 'item-1', isBlocking: true, autoResolutionMs: null,
  questions: [{ id: 'choice', question: binding.decision, options: [] }]
} };
const reply = { senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', text: ' 답변  \n☃ ' };
const mcpRequest = { jsonrpc: '2.0', id: 'mcp-1', method: 'tools/call', params: {
  name: 'ask_owner', arguments: { question: binding.decision }, _meta: { threadId: binding.threadId }
} };

function fixture(saved = null, send = async () => '6', respond = async () => {}, selected = binding) {
  const calls = { sends: [], responses: [], saves: [] };
  const store = { async load() { return structuredClone(saved); }, async save(s) {
    saved = structuredClone(s); calls.saves.push(saved);
  } };
  const bridge = new Bridge(selected, store, { async sendQuestion(text) {
    calls.sends.push(text); return send(text);
  } }, { async respond(id, result) { calls.responses.push({ id, result }); return respond(id, result); } });
  return { bridge, calls, saved: () => saved };
}

test('exact owner reply returns to the original request once', async () => {
  const f = fixture();
  await f.bridge.start(); f.bridge.onAttached();
  assert.equal(await f.bridge.onRequest(request), true);
  assert.equal(f.saved().phase, 'waiting');
  assert.deepEqual(f.calls.sends, [binding.decision]);
  assert.equal(await f.bridge.onReply({ ...reply, senderId: 'other' }), false);
  assert.equal(await f.bridge.onReply({ ...reply, chatId: 'other' }), false);
  assert.equal(await f.bridge.onReply({ ...reply, replyToMessageId: '5' }), false);
  assert.equal(await f.bridge.onReply(reply), true);
  assert.deepEqual(f.calls.responses, [{ id: 3, result: { answers: { choice: { answers: [reply.text] } } } }]);
  assert.equal(await f.bridge.onReply(reply), false);
  assert.equal(f.saved().nativeEffect, 'unknown');
  assert.equal(f.saved().goalEffect, 'pending');
  await f.bridge.onResolved({ method: 'serverRequest/resolved', params: { threadId: 'thread-1', requestId: 3 } });
  assert.equal(f.saved().nativeEffect, 'resolved_unconfirmed');
  assert.equal(f.saved().goalEffect, 'pending');
});

test('a private channel capability is not copied into persisted state', async () => {
  const f = fixture(null, undefined, undefined, { ...binding, capability: 'c'.repeat(64) });
  await f.bridge.start(); f.bridge.onAttached(); await f.bridge.onRequest(request);
  assert.equal(Object.hasOwn(f.saved().binding, 'capability'), false);
});

test('unanswered requests stay pending without timeout or fallback', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached();
  await f.bridge.onRequest({ ...request, params: { ...request.params, isBlocking: false } });
  assert.equal(f.saved().request.isBlocking, false);
  assert.equal(f.saved().phase, 'waiting');
  assert.deepEqual(f.calls.responses, []);
  assert.equal(await f.bridge.onRequest(request), false);
  assert.equal(f.calls.sends.length, 1);
});

test('different, timed, multi-question, and approval methods are not routed', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached();
  for (const candidate of [
    { ...request, method: 'item/commandExecution/requestApproval' },
    { ...request, params: { ...request.params, threadId: 'other' } },
    { ...request, params: { ...request.params, autoResolutionMs: 1000 } },
    { ...request, params: { ...request.params, questions: [...request.params.questions, ...request.params.questions] } },
    { ...request, params: { ...request.params, questions: [{ id: 'x', question: 'stale' }] } },
  ]) assert.equal(await f.bridge.onRequest(candidate), false);
  assert.equal(f.calls.sends.length, 0);
});

test('disconnect or restart cannot revive a pending request', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached(); await f.bridge.onRequest(request);
  const restartState = structuredClone(f.saved());
  await f.bridge.onDisconnect(); f.bridge.onAttached();
  assert.equal(await f.bridge.onReply(reply), false);
  assert.equal(await f.bridge.onRequest(request), false);
  assert.equal(f.saved().phase, 'needs_revalidation');
  const restarted = fixture(restartState); await restarted.bridge.start(); restarted.bridge.onAttached();
  assert.equal(await restarted.bridge.onRequest(request), false);
  assert.equal(await restarted.bridge.onReply(reply), false);
  assert.equal(restarted.calls.sends.length, 0);
});

test('corrupted persisted state cannot reset a request to ready', async () => {
  const f = fixture({ version: 1, binding, phase: 'ready', nativeEffect: 'unknown', goalEffect: 'pending' });
  await assert.rejects(f.bridge.start());
  assert.equal(f.calls.sends.length, 0);
});

test('uncertain send and uncertain native response never cause an automatic replay', async () => {
  const sendFailure = fixture(null, async () => { throw Error('lost response'); });
  await sendFailure.bridge.start(); sendFailure.bridge.onAttached();
  await sendFailure.bridge.onRequest(request);
  assert.equal(sendFailure.saved().phase, 'send_unknown');
  assert.equal(await sendFailure.bridge.onRequest(request), false);
  const replyFailure = fixture(null, undefined, async () => { throw Error('lost response'); });
  await replyFailure.bridge.start(); replyFailure.bridge.onAttached(); await replyFailure.bridge.onRequest(request);
  await replyFailure.bridge.onReply(reply);
  assert.equal(replyFailure.saved().nativeEffect, 'unknown');
  assert.equal(await replyFailure.bridge.onReply(reply), false);
  assert.equal(replyFailure.calls.responses.length, 1);
});

test('native cleanup before answer rejects the old Telegram reply', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached(); await f.bridge.onRequest(request);
  await f.bridge.onResolved({ method: 'serverRequest/resolved', params: { threadId: 'thread-1', requestId: 3 } });
  assert.equal(f.saved().phase, 'request_cleared');
  assert.equal(await f.bridge.onReply(reply), false);
  assert.equal(f.calls.responses.length, 0);
});

test('separate reported native and Goal effects never trigger a replay', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached(); await f.bridge.onRequest(request);
  await f.bridge.onReply(reply);
  const goal = { goalId: binding.goalId, goal: binding.goal, previousEventSha256: binding.expectedEventSha256,
    eventSha256: 'd'.repeat(64), decisions: [{ id: binding.decision, text: binding.decision,
      disposition: 'accepted', resultRefs: ['result-ref'] }] };
  assert.equal(await f.bridge.onGoalObservation({ record: { ...goal, previousEventSha256: 'e'.repeat(64) },
    resultRef: 'result-ref' }), false);
  assert.equal(await f.bridge.onGoalObservation({ record: goal, resultRef: 'result-ref' }), true);
  assert.equal(f.saved().goalEffect, 'recorded_reported');
  assert.equal(f.saved().nativeEffect, 'unknown');
  assert.equal(await f.bridge.onReply(reply), false);
  assert.equal(await f.bridge.onNativeObservation({ threadId: 'thread-1', requestId: 3, outcome: 'accepted',
    answerSha256: 'f'.repeat(64), evidenceRef: 'native-result' }), false);
  assert.equal(await f.bridge.onNativeObservation({ threadId: 'thread-1', requestId: 3, outcome: 'accepted',
    answerSha256: createHash('sha256').update(reply.text).digest('hex'), evidenceRef: 'native-result' }), true);
  assert.equal(f.saved().phase, 'reported_complete');
  assert.equal(f.saved().answer, undefined);
  assert.equal(f.calls.responses.length, 1);
});

test('an actual bound MCP request retains its ID and exact answer', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached();
  assert.equal(await f.bridge.onMcpRequest(mcpRequest), true);
  assert.equal(f.saved().request.transport, 'mcp');
  assert.equal(await f.bridge.onReply(reply), true);
  assert.deepEqual(f.calls.responses, [{ id: 'mcp-1', result: { content: [{ type: 'text', text: reply.text }] } }]);
  assert.equal(f.saved().nativeEffect, 'unknown');
  assert.equal(f.saved().goalEffect, 'pending');
  const restarted = fixture(f.saved());
  await restarted.bridge.start(); restarted.bridge.onAttached();
  assert.equal(await restarted.bridge.onReply(reply), false);
});

test('unbound MCP methods, threads, questions and cancellation IDs cannot take the request', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached();
  for (const candidate of [
    { ...mcpRequest, method: 'other' },
    { ...mcpRequest, params: { ...mcpRequest.params, name: 'other' } },
    { ...mcpRequest, params: { ...mcpRequest.params, _meta: { threadId: 'other' } } },
    { ...mcpRequest, params: { ...mcpRequest.params, arguments: { question: 'different' } } },
  ]) assert.equal(await f.bridge.onMcpRequest(candidate), false);
  assert.equal(await f.bridge.onMcpRequest(mcpRequest), true);
  assert.equal(await f.bridge.onMcpCancelled({ jsonrpc: '2.0', method: 'notifications/cancelled',
    params: { requestId: 'other' } }), false);
  assert.equal(f.saved().phase, 'waiting');
  assert.equal(await f.bridge.onMcpCancelled({ jsonrpc: '2.0', method: 'notifications/cancelled',
    params: { requestId: 'mcp-1' } }), true);
  assert.equal(f.saved().phase, 'request_cleared');
  assert.equal(await f.bridge.onReply(reply), false);
  assert.deepEqual(f.calls.responses, []);
});

test('MCP cancellation after a response attempt retains uncertainty', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached();
  await f.bridge.onMcpRequest(mcpRequest); await f.bridge.onReply(reply);
  assert.equal(await f.bridge.onMcpCancelled({ jsonrpc: '2.0', method: 'notifications/cancelled',
    params: { requestId: 'mcp-1' } }), true);
  assert.equal(f.saved().phase, 'stale');
  assert.equal(f.saved().nativeEffect, 'unknown');
  const restarted = fixture(f.saved()); await restarted.bridge.start();
  assert.equal(restarted.saved().phase, 'stale');
});

test('routing evidence is bounded, contains only fixed reasons, and stops after cancellation', async () => {
  const f = fixture(); await f.bridge.start(); f.bridge.onAttached(); await f.bridge.onMcpRequest(mcpRequest);
  for (let i = 0; i < 260; i++) await f.bridge.onRouting('missing_reply_to');
  await f.bridge.onRouting('other_reply_to'); await f.bridge.onRouting('not-a-reason');
  assert.deepEqual(f.saved().routingReasons, { missing_reply_to: 255, other_reply_to: 1 });
  await f.bridge.onMcpCancelled({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 'mcp-1' } });
  await f.bridge.onRouting('matched_reply');
  assert.deepEqual(f.saved().routingReasons, { missing_reply_to: 255, other_reply_to: 1 });
});
