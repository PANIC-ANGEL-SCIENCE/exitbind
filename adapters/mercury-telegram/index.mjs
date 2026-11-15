// One optional owner-bound bridge. No model, bot poller, or Goal writer lives here.
import { constants } from 'node:fs';
import { open, lstat, rename, unlink, chmod, mkdir } from 'node:fs/promises';
import { join, isAbsolute } from 'node:path';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { createServer } from 'node:net';

const MAX_STATE = 64 * 1024;
const ID = /^[A-Za-z0-9_.:-]{1,256}$/;
const HASH = /^[0-9a-f]{64}$/;
const ROUTING_REASONS = ['matched_reply', 'missing_reply_to', 'other_reply_to'];
const MAX_ROUTING_COUNT = 255;

function requireBinding(b) {
  if (!b || b.version !== 1 || b.kind !== 'clarification' ||
      ![b.threadId, b.goalId, b.senderId, b.chatId].every(x => typeof x === 'string' && ID.test(x)) ||
      typeof b.goal !== 'string' || !b.goal.trim() || typeof b.decision !== 'string' ||
      !b.decision.trim() || b.decision !== b.decision.trim() || !HASH.test(b.expectedEventSha256)) {
    throw new Error('invalid owner clarification binding');
  }
  const { version, kind, threadId, goalId, goal, decision, expectedEventSha256, senderId, chatId } = b;
  return Object.freeze({ version, kind, threadId, goalId, goal, decision, expectedEventSha256, senderId, chatId });
}

function requestKey(r) {
  return JSON.stringify([r.id, r.threadId, r.turnId, r.itemId, r.questionId]);
}

function validateSaved(s, binding) {
  const phases = ['ready', 'sending', 'waiting', 'send_unknown', 'needs_revalidation',
    'answer_submitted', 'awaiting_observation', 'request_cleared', 'stale', 'reported_complete'];
  if (!s || s.version !== 1 || JSON.stringify(s.binding) !== JSON.stringify(binding) ||
      !phases.includes(s.phase) || !['none', 'unknown', 'resolved_unconfirmed', 'not_sent', 'accepted_reported'].includes(s.nativeEffect) ||
      !['pending', 'recorded_reported'].includes(s.goalEffect)) throw new Error('saved bridge state is invalid or belongs to another binding');
  if (s.routingReasons !== undefined && (!s.routingReasons || typeof s.routingReasons !== 'object' ||
      Array.isArray(s.routingReasons) || Object.entries(s.routingReasons).some(([key, value]) =>
        !ROUTING_REASONS.includes(key) || !Number.isInteger(value) || value < 1 || value > MAX_ROUTING_COUNT))) {
    throw new Error('saved routing evidence is invalid');
  }
  if (s.phase === 'ready') {
    if (s.request || s.messageId || s.replyId || s.answer || s.routingReasons || s.nativeEffect !== 'none') {
      throw new Error('saved ready state is inconsistent');
    }
    return;
  }
  const r = s.request;
  const native = r?.transport === undefined && typeof r.turnId === 'string' &&
    typeof r.itemId === 'string' && typeof r.questionId === 'string' && typeof r.isBlocking === 'boolean';
  const mcp = r?.transport === 'mcp' && r.rendered === binding.decision;
  if (!r || !['string', 'number'].includes(typeof r.id) || r.threadId !== binding.threadId ||
      (!native && !mcp) || typeof r.rendered !== 'string' || !r.rendered.startsWith(binding.decision)) {
    throw new Error('saved request is invalid');
  }
  if (['waiting', 'request_cleared', 'answer_submitted', 'awaiting_observation', 'stale', 'reported_complete'].includes(s.phase) &&
      (typeof s.messageId !== 'string' || !ID.test(s.messageId))) throw new Error('saved message is invalid');
  if (['answer_submitted', 'awaiting_observation', 'stale'].includes(s.phase) &&
      (typeof s.replyId !== 'string' || !ID.test(s.replyId) || typeof s.answer !== 'string' || !s.answer.length ||
       !HASH.test(s.answerSha256) ||
       s.nativeEffect === 'none')) throw new Error('saved answer is invalid');
  if (s.phase === 'reported_complete' && (s.nativeEffect !== 'accepted_reported' ||
      s.goalEffect !== 'recorded_reported' || !HASH.test(s.answerSha256) || s.answer !== undefined)) {
    throw new Error('saved completion observations are invalid');
  }
  if (s.nativeEffect === 'accepted_reported' &&
      (typeof s.nativeEvidenceRef !== 'string' || !s.nativeEvidenceRef.trim())) {
    throw new Error('saved native observation is invalid');
  }
  if (s.goalEffect === 'recorded_reported' &&
      (!HASH.test(s.goalEventSha256) || typeof s.goalResultRef !== 'string' || !s.goalResultRef.trim())) {
    throw new Error('saved Goal observation is invalid');
  }
}

function currentRequest(message, binding) {
  if (message?.method !== 'item/tool/requestUserInput' ||
      !['string', 'number'].includes(typeof message.id)) return null;
  const p = message.params;
  if (!p || p.threadId !== binding.threadId || typeof p.turnId !== 'string' ||
      typeof p.itemId !== 'string' || typeof p.isBlocking !== 'boolean' ||
      p.autoResolutionMs !== null || !Array.isArray(p.questions) || p.questions.length !== 1) return null;
  const q = p.questions[0];
  if (!q || typeof q.id !== 'string' || !ID.test(q.id) || q.question !== binding.decision) return null;
  const options = q.options ?? [];
  if (!Array.isArray(options) || options.some(o => !o || typeof o.label !== 'string' ||
      typeof o.description !== 'string')) return null;
  const rendered = [q.question, ...options.map(o => `${o.label}: ${o.description}`)].join('\n');
  if (!rendered || rendered.length > 3900) return null;
  return { id: message.id, threadId: p.threadId, turnId: p.turnId, itemId: p.itemId,
    questionId: q.id, isBlocking: p.isBlocking, rendered };
}

// This is an actual MCP tools/call from the existing client, not an invented
// Codex serverRequest. The MCP connection owns its request ID and lifetime.
function currentMcpRequest(message, binding) {
  const p = message?.params;
  if (message?.jsonrpc !== '2.0' || message.method !== 'tools/call' ||
      !['string', 'number'].includes(typeof message.id) || p?.name !== 'ask_owner' ||
      p?._meta?.threadId !== binding.threadId || p?.arguments?.question !== binding.decision ||
      binding.decision.length > 3900) return null;
  return { transport: 'mcp', id: message.id, threadId: binding.threadId, rendered: binding.decision };
}

// Save-before-effect is intentional: a crash around an external effect is unknown,
// never permission to repeat it. Store implementations must make save durable.
export class Bridge {
  constructor(binding, store, channel, native) {
    this.binding = requireBinding(binding);
    this.store = store;
    this.channel = channel;
    this.native = native;
    this.serial = Promise.resolve();
    this.state = null;
    this.attached = false;
  }

  async start() {
    const saved = await this.store.load();
    if (saved) validateSaved(saved, this.binding);
    this.state = saved ?? { version: 1, binding: this.binding, phase: 'ready',
      nativeEffect: 'none', goalEffect: 'pending' };
    if (saved && ['sending', 'waiting'].includes(saved.phase)) {
      this.state.phase = 'needs_revalidation';
      await this.store.save(this.state);
    }
  }

  enqueue(fn) {
    const result = this.serial.then(fn);
    this.serial = result.catch(() => {});
    return result;
  }

  onAttached() { this.attached = true; }

  isQuestionActive() { return this.attached && ['sending', 'waiting'].includes(this.state?.phase); }

  onRouting(reason) {
    return this.enqueue(async () => {
      if (!this.isQuestionActive() || !ROUTING_REASONS.includes(reason)) return;
      this.state.routingReasons ??= {};
      this.state.routingReasons[reason] = Math.min(MAX_ROUTING_COUNT, (this.state.routingReasons[reason] ?? 0) + 1);
      await this.store.save(this.state);
    });
  }

  onRequest(message) {
    return this.onBoundRequest(currentRequest(message, this.binding));
  }

  onMcpRequest(message) {
    return this.onBoundRequest(currentMcpRequest(message, this.binding));
  }

  onBoundRequest(request) {
    return this.enqueue(async () => {
      if (!this.attached || !request || this.state.phase !== 'ready') return false;
      this.state.request = request;
      this.state.phase = 'sending';
      await this.store.save(this.state);
      try {
        const messageId = await this.channel.sendQuestion(request.rendered);
        if (typeof messageId !== 'string' || !ID.test(messageId)) throw new Error('missing message id');
        this.state.messageId = messageId;
        this.state.phase = 'waiting';
        await this.store.save(this.state);
      } catch {
        this.state.phase = 'send_unknown';
        await this.store.save(this.state);
      }
      return true;
    });
  }

  onReply(reply) {
    return this.enqueue(async () => {
      if (!this.attached || this.state.phase !== 'waiting' || !reply ||
          reply.senderId !== this.binding.senderId || reply.chatId !== this.binding.chatId ||
          reply.replyToMessageId !== this.state.messageId || typeof reply.messageId !== 'string' ||
          !ID.test(reply.messageId) || typeof reply.text !== 'string' || reply.text.length === 0) return false;
      this.state.replyId = reply.messageId;
      this.state.answer = reply.text;
      this.state.answerSha256 = createHash('sha256').update(reply.text).digest('hex');
      this.state.nativeEffect = 'unknown';
      this.state.phase = 'answer_submitted';
      await this.store.save(this.state);
      const request = this.state.request;
      try {
        const result = request.transport === 'mcp'
          ? { content: [{ type: 'text', text: reply.text }] }
          : { answers: { [request.questionId]: { answers: [reply.text] } } };
        await this.native.respond(request.id, result);
      } catch {
        // A failed write can still have reached the server. Observe; do not resend.
      }
      return true;
    });
  }

  onResolved(message) {
    return this.enqueue(async () => {
      if (message?.method !== 'serverRequest/resolved' || !this.state.request || this.state.request.transport === 'mcp' ||
          message.params?.threadId !== this.binding.threadId ||
          message.params?.requestId !== this.state.request.id) return false;
      if (this.state.phase === 'answer_submitted') {
        if (this.state.nativeEffect === 'unknown') this.state.nativeEffect = 'resolved_unconfirmed';
        this.state.phase = 'awaiting_observation';
      } else if (this.state.phase === 'waiting') {
        this.state.phase = 'request_cleared';
      } else return false;
      await this.store.save(this.state);
      return true;
    });
  }

  onMcpCancelled(message) {
    return this.enqueue(async () => {
      if (message?.jsonrpc !== '2.0' || message.method !== 'notifications/cancelled' ||
          this.state.request?.transport !== 'mcp' || message.params?.requestId !== this.state.request.id) return false;
      if (this.state.phase === 'waiting') this.state.phase = 'request_cleared';
      else if (['answer_submitted', 'awaiting_observation'].includes(this.state.phase) &&
          this.state.nativeEffect !== 'accepted_reported') this.state.phase = 'stale';
      else return false;
      await this.store.save(this.state);
      return true;
    });
  }

  onDisconnect() {
    this.attached = false;
    return this.enqueue(async () => {
      if (['sending', 'waiting'].includes(this.state.phase)) {
        this.state.phase = 'needs_revalidation';
        await this.store.save(this.state);
      }
    });
  }

  onNativeNotSent(id) {
    return this.enqueue(async () => {
      if (this.state.phase !== 'answer_submitted' || this.state.request?.id !== id) return false;
      this.state.nativeEffect = 'not_sent';
      this.state.phase = 'stale';
      await this.store.save(this.state);
      return true;
    });
  }

  onNativeObservation(observation) {
    return this.enqueue(async () => {
      if (!['answer_submitted', 'awaiting_observation'].includes(this.state.phase) ||
          !observation || observation.threadId !== this.binding.threadId ||
          observation.requestId !== this.state.request?.id || observation.outcome !== 'accepted' ||
          observation.answerSha256 !== this.state.answerSha256 ||
          typeof observation.evidenceRef !== 'string' || !observation.evidenceRef.trim()) return false;
      this.state.nativeEffect = 'accepted_reported';
      this.state.nativeEvidenceRef = observation.evidenceRef;
      this.finishReported();
      await this.store.save(this.state);
      return true;
    });
  }

  onGoalObservation(observation) {
    return this.enqueue(async () => {
      const record = observation?.record;
      const ref = observation?.resultRef;
      if (!['answer_submitted', 'awaiting_observation'].includes(this.state.phase) ||
          !record || record.goalId !== this.binding.goalId || record.goal !== this.binding.goal ||
          record.previousEventSha256 !== this.binding.expectedEventSha256 || !HASH.test(record.eventSha256) ||
          typeof ref !== 'string' || !ref.trim() || !Array.isArray(record.decisions) ||
          !record.decisions.some(item => item?.id === this.binding.decision && item.text === this.binding.decision &&
            item.disposition === 'accepted' && Array.isArray(item.resultRefs) && item.resultRefs.includes(ref))) return false;
      this.state.goalEffect = 'recorded_reported';
      this.state.goalEventSha256 = record.eventSha256;
      this.state.goalResultRef = ref;
      this.finishReported();
      await this.store.save(this.state);
      return true;
    });
  }

  finishReported() {
    if (this.state.nativeEffect === 'accepted_reported' && this.state.goalEffect === 'recorded_reported') {
      this.state.phase = 'reported_complete';
      delete this.state.answer;
    }
  }
}

async function privateFile(path) {
  const f = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const s = await f.stat();
    if (!s.isFile() || s.uid !== process.getuid() || (s.mode & 0o077) !== 0 || s.size > MAX_STATE) {
      throw new Error('private file is unavailable or unsafe');
    }
    return JSON.parse(await f.readFile({ encoding: 'utf8' }));
  } finally { await f.close(); }
}

export class PrivateStore {
  constructor(dir) { this.dir = dir; this.path = join(dir, 'owner-bridge-state.json'); }
  async checkDirectory() {
    const s = await lstat(this.dir);
    if (!s.isDirectory() || s.uid !== process.getuid() || (s.mode & 0o077) !== 0) {
      throw new Error('private state directory is unavailable or unsafe');
    }
  }
  async load() {
    await this.checkDirectory();
    try { return await privateFile(this.path); }
    catch (e) { if (e.code === 'ENOENT') return null; throw e; }
  }
  async lock() {
    await this.checkDirectory();
    this.lockPath = join(this.dir, '.owner-bridge.lock');
    this.lockFile = await open(this.lockPath,
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
    this.lockToken = randomBytes(32).toString('hex');
    try {
      await this.lockFile.writeFile(this.lockToken);
      await this.lockFile.sync();
    } catch (error) {
      await this.lockFile.close().catch(() => {});
      this.lockFile = null;
      await unlink(this.lockPath).catch(() => {});
      throw error;
    }
  }
  async unlock() {
    if (!this.lockFile) return;
    await this.lockFile.close();
    this.lockFile = null;
    const f = await open(this.lockPath, constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
      if ((await f.readFile({ encoding: 'utf8' })) !== this.lockToken) return;
    } finally { await f.close(); }
    await unlink(this.lockPath);
  }
  async save(value) {
    await this.checkDirectory();
    const bytes = JSON.stringify(value);
    if (Buffer.byteLength(bytes) > MAX_STATE) throw new Error('bridge state exceeds limit');
    const temp = join(this.dir, `.owner-bridge-${randomBytes(16).toString('hex')}`);
    const f = await open(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
    try { await f.writeFile(bytes); await f.sync(); } finally { await f.close(); }
    try {
      await rename(temp, this.path);
      const dir = await open(this.dir, constants.O_RDONLY | constants.O_DIRECTORY);
      try { await dir.sync(); } finally { await dir.close(); }
    } catch (e) { await unlink(temp).catch(() => {}); throw e; }
  }
}

// Mercury creates its plugin data directory with the host umask. Put private
// records in our own restricted child without changing that existing directory.
export async function privateDataDirectory(base) {
  const parent = await lstat(base);
  if (!parent.isDirectory() || parent.uid !== process.getuid() || (parent.mode & 0o022) !== 0) {
    throw new Error('plugin data directory is unavailable or unsafe');
  }
  const dir = join(base, 'owner-bridge-private');
  try { await mkdir(dir, { mode: 0o700 }); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  const child = await lstat(dir);
  if (!child.isDirectory() || child.uid !== process.getuid() || (child.mode & 0o077) !== 0) {
    throw new Error('private state directory is unavailable or unsafe');
  }
  return dir;
}

// A host may supply a shorter existing directory for AF_UNIX without moving
// private records or changing the permissions of an existing directory.
export async function privateSocketDirectory(defaultDir, selected) {
  const dir = selected === undefined ? defaultDir : selected;
  if (typeof dir !== 'string' || !isAbsolute(dir)) throw new Error('absolute private socket directory required');
  const stat = await lstat(dir);
  if (!stat.isDirectory() || stat.uid !== process.getuid() || (stat.mode & 0o077) !== 0) {
    throw new Error('private socket directory is unavailable or unsafe');
  }
  return dir;
}

// The original client forwards only its own live request. This endpoint never
// starts, resumes, forks, or discovers a Codex thread.
export async function listenForNative(dir, capability, bridge) {
  if (typeof capability !== 'string' || !HASH.test(capability)) throw new Error('private capability required');
  const socketPath = join(dir, 'owner-bridge.sock');
  // AF_UNIX path limits vary by host. Refuse before Node can silently truncate.
  if (Buffer.byteLength(socketPath) > 100) throw new Error('private bridge socket path is too long');
  let peer = null;
  let attachedOnce = false;
  const sockets = new Set();
  const write = value => new Promise((resolve, reject) => {
    if (!peer?.writable) return reject(new Error('native client unavailable'));
    peer.write(`${JSON.stringify(value)}\n`, e => e ? reject(e) : resolve());
  });
  const server = createServer(socket => {
    if (attachedOnce || peer) { socket.destroy(); return; }
    sockets.add(socket);
    let buffer = '';
    let authenticated = false;
    socket.setEncoding('utf8');
    socket.on('error', () => {});
    socket.on('close', () => {
      sockets.delete(socket);
      if (peer === socket) { peer = null; void bridge.onDisconnect(); }
    });
    socket.on('data', chunk => {
      buffer += chunk;
      if (Buffer.byteLength(buffer) > MAX_STATE) { socket.destroy(); return; }
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
        let m;
        try { m = JSON.parse(line); } catch { socket.destroy(); return; }
        if (!authenticated) {
          const token = typeof m?.capability === 'string' && HASH.test(m.capability)
            ? Buffer.from(m.capability, 'hex') : Buffer.alloc(0);
          if (attachedOnce || peer || m?.type !== 'attach' || m.threadId !== bridge.binding.threadId || token.length !== 32 ||
              !timingSafeEqual(token, Buffer.from(capability, 'hex'))) { socket.destroy(); return; }
          authenticated = true; attachedOnce = true; peer = socket; bridge.onAttached();
          socket.write('{"type":"attached"}\n');
        } else if (m.type === 'request') {
          void bridge.onRequest(m.message).then(accepted => write({ type: 'requestObserved',
            id: m.message?.id, accepted })).catch(() => write({ type: 'requestUncertain',
              id: m.message?.id }).catch(() => {}));
        } else if (m.type === 'mcpRequest') {
          void bridge.onMcpRequest(m.message).then(accepted => write({ type: 'requestObserved',
            id: m.message?.id, accepted })).catch(() => write({ type: 'requestUncertain',
              id: m.message?.id }).catch(() => {}));
        } else if (m.type === 'mcpCancelled') {
          void bridge.onMcpCancelled(m.message).catch(() => {});
        } else if (m.type === 'resolved') {
          void bridge.onResolved(m.message).catch(() => {});
        } else if (m.type === 'responseNotSent') {
          void bridge.onNativeNotSent(m.id).catch(() => {});
        } else if (m.type === 'nativeObservation') {
          void bridge.onNativeObservation(m.observation).catch(() => {});
        } else if (m.type === 'goalObservation') {
          void bridge.onGoalObservation(m.observation).catch(() => {});
        } else socket.destroy();
      }
    });
  });
  server.on('error', () => {});
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(socketPath, () => { server.removeListener('error', reject); resolve(); });
  });
  let socketStat;
  try {
    socketStat = await lstat(socketPath);
    await chmod(socketPath, 0o600);
  } catch (error) {
    await new Promise(resolve => server.close(resolve));
    const current = await lstat(socketPath).catch(() => null);
    if (socketStat && current && current.ino === socketStat.ino && current.dev === socketStat.dev) {
      await unlink(socketPath).catch(() => {});
    }
    throw error;
  }
  return {
    socketPath,
    respond: (id, result) => write({ type: 'response', id, result }),
    async close() {
      for (const socket of sockets) socket.destroy();
      await new Promise(resolve => server.close(resolve));
      const current = await lstat(socketPath).catch(() => null);
      if (current && current.ino === socketStat.ino && current.dev === socketStat.dev) await unlink(socketPath);
    }
  };
}

export async function activate(ctx) {
  if (!ctx.ownerBridge || typeof ctx.ownerBridge.bind !== 'function') {
    throw new Error('Mercury owner bridge extension is unavailable');
  }
  const bindingPath = ctx.settings.get('bindingFile');
  if (typeof bindingPath !== 'string' || !isAbsolute(bindingPath)) throw new Error('private binding file required');
  const supplied = await privateFile(bindingPath);
  const binding = requireBinding(supplied);
  if (typeof supplied.capability !== 'string' || !HASH.test(supplied.capability)) throw new Error('private capability required');
  const dataDir = await privateDataDirectory(ctx.plugin.dataDir);
  const socketDir = await privateSocketDirectory(dataDir, supplied.socketDirectory);
  const store = new PrivateStore(dataDir);
  await store.lock();
  ctx.onDispose(() => store.unlock());
  let native;
  let channel;
  const bridge = new Bridge(binding, store, { sendQuestion: text => channel.sendQuestion(text) },
    { respond: (id, result) => native.respond(id, result) });
  await bridge.start();
  channel = ctx.ownerBridge.bind({ senderId: binding.senderId, chatId: binding.chatId,
    messageId: bridge.state.messageId,
    sendUncertain: !bridge.state.messageId && bridge.state.phase !== 'ready',
    isQuestionActive: () => bridge.isQuestionActive(), onRouting: reason => bridge.onRouting(reason),
    onReply: reply => bridge.onReply(reply) });
  if (channel?.routingGuard !== 1) {
    try { channel?.dispose(); } catch { /* reject even if the incompatible host cannot clean up */ }
    throw new Error('Mercury owner bridge routing guard is unavailable');
  }
  ctx.onDispose(() => channel.dispose());
  native = await listenForNative(socketDir, supplied.capability, bridge);
  ctx.onDispose(() => native.close());
}

export { currentMcpRequest, currentRequest, requestKey };
