import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { bindOwnerBridge, clearOwnerBridgeHost, isOwnerBridgeQuestion, routeOwnerMessage, setOwnerBridgeHost } from '../mercury-owner-bridge.ts';
import { Bridge } from '../index.mjs';

const reply = { senderId: '42', chatId: '44', messageId: '7', replyToMessageId: '6', replyToIsBridgeQuestion: true, text: '  answer ☃ ' };
function setup(send = async () => '6') {
  const received = [];
  const reasons = [];
  let active = true;
  const host = { isOwner: (senderId, chatId) => senderId === '42' && chatId === '44', send };
  setOwnerBridgeHost(host);
  const bound = bindOwnerBridge({ senderId: '42', chatId: '44', isQuestionActive: () => active,
    onRouting: async reason => { reasons.push(reason); }, onReply: async r => { received.push(r); return true; } });
  return { host, bound, received, reasons, setActive(value) { active = value; }, close() { bound.dispose(); clearOwnerBridgeHost(host); } };
}

test('only the selected owner reply is consumed; unrelated Mercury messages pass through', async () => {
  const f = setup();
  try {
    assert.equal(f.bound.routingGuard, 1);
    assert.equal(await f.bound.sendQuestion('Question?'), '6');
    assert.equal(await routeOwnerMessage({ ...reply, senderId: 'other' }), false);
    assert.equal(await routeOwnerMessage({ ...reply, chatId: 'other' }), false);
    assert.equal(await routeOwnerMessage({ ...reply, replyToMessageId: 'other', replyToIsBridgeQuestion: false }), true);
    assert.equal(await routeOwnerMessage(reply), true);
    const { replyToIsBridgeQuestion: _, ...expected } = reply;
    assert.deepEqual(f.received, [expected]);
    assert.deepEqual(f.reasons, ['other_reply_to', 'matched_reply']);
    // The adapter rejects duplicates; the host still consumes replies to its message.
    assert.equal(await routeOwnerMessage(reply), true);
  } finally { f.close(); }
  assert.equal(await routeOwnerMessage(reply), false);
});

test('an older adapter without the required routing callbacks is rejected', () => {
  assert.throws(() => bindOwnerBridge({ senderId: '42', chatId: '44', async onReply() { return true; } }),
    /invalid owner bridge binding/);
});

test('reply arriving during send waits for the actual Telegram message id', async () => {
  let finish;
  const f = setup(() => new Promise(resolve => { finish = resolve; }));
  try {
    const sent = f.bound.sendQuestion('Question?');
    const incoming = routeOwnerMessage(reply);
    await Promise.resolve();
    finish('6');
    assert.equal(await sent, '6');
    assert.equal(await incoming, true);
    const { replyToIsBridgeQuestion: _, ...expected } = reply;
    assert.deepEqual(f.received, [expected]);
  } finally { f.close(); }
});

test('uncertain send quarantines only a marked bot-question reply', async () => {
  const f = setup(async () => { throw new Error('ambiguous network failure'); });
  try {
    await assert.rejects(f.bound.sendQuestion('Question?'));
    f.setActive(false);
    assert.equal(await routeOwnerMessage(reply), true);
    assert.equal(await routeOwnerMessage({ ...reply, replyToIsBridgeQuestion: false }), false);
    assert.equal(f.received.length, 0);
    assert.equal(await routeOwnerMessage({ ...reply, senderId: 'other' }), false);
  } finally { f.close(); }
});

test('a bot-authored bridge question stays recognizable after the plugin stops', async () => {
  let sent;
  const f = setup(async (_sender, _chat, text) => { sent = text; return '6'; });
  try { await f.bound.sendQuestion('Question?'); } finally { f.close(); }
  assert.equal(isOwnerBridgeQuestion(true, sent), true);
  assert.equal(isOwnerBridgeQuestion(false, sent), false);
  assert.equal(isOwnerBridgeQuestion(true, '[Exitbind question fake]\nQuestion?'), false);
});

test('SDK rejection records only a bounded reason in the attached bridge state', async () => {
  const binding = { version: 1, kind: 'clarification', threadId: 'thread', goalId: 'goal', goal: 'Finish',
    decision: 'Question?', expectedEventSha256: 'a'.repeat(64), senderId: '42', chatId: '44' };
  let saved, bound, responseCount = 0, sendCount = 0;
  const store = { async load() { return null; }, async save(value) { saved = structuredClone(value); } };
  const host = { isOwner: (sender, chat) => sender === '42' && chat === '44',
    async send() { sendCount += 1; return '6'; } };
  const bridge = new Bridge(binding, store, { sendQuestion: text => bound.sendQuestion(text) },
    { async respond() { responseCount += 1; } });
  setOwnerBridgeHost(host);
  try {
    await bridge.start(); bridge.onAttached();
    bound = bindOwnerBridge({ senderId: '42', chatId: '44', isQuestionActive: () => bridge.isQuestionActive(),
      onRouting: reason => bridge.onRouting(reason), onReply: value => bridge.onReply(value) });
    const request = { jsonrpc: '2.0', id: 'request', method: 'tools/call', params: { name: 'ask_owner',
      arguments: { question: binding.decision }, _meta: { threadId: binding.threadId } } };
    assert.equal(await bridge.onMcpRequest(request), true);
    const rejectedText = 'private uncorrelated fixture text';
    assert.equal(await routeOwnerMessage({ senderId: '42', chatId: '44', messageId: '7',
      replyToIsBridgeQuestion: false, text: rejectedText }), true);
    assert.deepEqual(saved.routingReasons, { missing_reply_to: 1 });
    assert.equal(JSON.stringify(saved).includes(rejectedText), false);
    assert.equal(responseCount, 0); assert.equal(sendCount, 2);
    await bridge.onMcpCancelled({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: 'request' } });
    assert.equal(await routeOwnerMessage({ senderId: '42', chatId: '44', messageId: '8',
      replyToIsBridgeQuestion: false, text: rejectedText }), false);
    assert.deepEqual(saved.routingReasons, { missing_reply_to: 1 });
  } finally { bound?.dispose(); clearOwnerBridgeHost(host); }
});

test('the shipped patch contains the exact host source exercised above', async () => {
  const patch = await readFile(new URL('../mercury-sdk.patch', import.meta.url), 'utf8');
  const body = patch.split('+++ b/src/plugins/owner-bridge.ts\n')[1];
  assert.ok(body, 'host boundary missing from Mercury patch');
  const source = body.split('\n').filter(line => line.startsWith('+') && !line.startsWith('+++'))
    .map(line => line.slice(1)).join('\n') + '\n';
  assert.equal(source, await readFile(new URL('../mercury-owner-bridge.ts', import.meta.url), 'utf8'));
});

test('the manifest requests the narrow owner capability and requires a private binding', async () => {
  const manifest = JSON.parse(await readFile(new URL('../mercury-plugin.json', import.meta.url), 'utf8'));
  assert.equal(manifest.kind, 'in-process');
  assert.equal(manifest.permissions.telegramOwnerBridge, true);
  assert.deepEqual(manifest.permissions.network, []);
  assert.deepEqual(manifest.permissions.secrets, []);
  assert.equal(manifest.settings.fields.bindingFile.required, true);
  assert.equal(manifest.settings.fields.bindingFile.setup, true);
  const patch = await readFile(new URL('../mercury-sdk.patch', import.meta.url), 'utf8');
  assert.match(patch, /ownerBridge: m\.permissions\.telegramOwnerBridge \?/);
});
