import test from 'node:test';
import assert from 'node:assert/strict';
import { chmod, lstat, mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { activate, PrivateStore, listenForNative, privateDataDirectory, privateSocketDirectory } from '../index.mjs';
import { bindOwnerBridge } from '../mercury-owner-bridge.ts';
import { bindOwnerBridge as bindLegacyOwnerBridge } from './fixtures/legacy-owner-bridge.ts';

test('creates a private child under Mercury’s ordinary non-writable data directory', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-data-fixture-'));
  try {
    await chmod(dir, 0o755);
    const privateDir = await privateDataDirectory(dir);
    assert.equal((await lstat(dir)).mode & 0o777, 0o755);
    assert.equal((await lstat(privateDir)).mode & 0o077, 0);
    await chmod(dir, 0o777);
    await assert.rejects(privateDataDirectory(dir));
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('state is private, durable on readback, and single-writer', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-private-fixture-'));
  const first = new PrivateStore(dir), second = new PrivateStore(dir);
  try {
    await first.lock();
    await assert.rejects(second.lock());
    await first.save({ phase: 'waiting', answer: '  exact  ' });
    assert.deepEqual(await first.load(), { phase: 'waiting', answer: '  exact  ' });
    await first.unlock();
    await second.lock();
    await second.unlock();
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('state reader refuses a symlink instead of following it', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'r67-link-fixture-'));
  try {
    await writeFile(join(dir, 'other'), '{}', { mode: 0o600 });
    await symlink(join(dir, 'other'), join(dir, 'owner-bridge-state.json'));
    await assert.rejects(new PrivateStore(dir).load());
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('a long socket path is refused before attempting to listen', async () => {
  const root = await mkdtemp(join(tmpdir(), 'r67-long-fixture-'));
  try {
    const dir = join(root, 'x'.repeat(90));
    await mkdir(dir, { mode: 0o700 });
    await assert.rejects(listenForNative(dir, 'a'.repeat(64), {}), /socket path is too long/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('an explicitly selected short private directory can hold the live socket', async () => {
  const root = await mkdtemp(join(tmpdir(), 'r67-short-fixture-'));
  const socketDir = await mkdtemp(join(tmpdir(), 'r67-socket-'));
  const disposers = [];
  try {
    const dataDir = join(root, 'x'.repeat(90));
    await mkdir(dataDir, { mode: 0o700 });
    const bindingFile = join(root, 'binding.json');
    await writeFile(bindingFile, JSON.stringify({ version: 1, kind: 'clarification', threadId: 'thread',
      goalId: 'goal', goal: 'Goal', decision: 'Question?', expectedEventSha256: 'a'.repeat(64),
      senderId: '1', chatId: '2', capability: 'b'.repeat(64), socketDirectory: socketDir }), { mode: 0o600 });
    const ctx = { plugin: { dataDir }, settings: { get: () => bindingFile },
      ownerBridge: { bind: bindOwnerBridge }, onDispose: fn => disposers.push(fn) };
    await activate(ctx);
    assert.ok((await lstat(join(socketDir, 'owner-bridge.sock'))).isSocket());
    assert.equal((await lstat(socketDir)).mode & 0o777, 0o700);
  } finally {
    for (const dispose of disposers.reverse()) await dispose();
    await rm(root, { recursive: true, force: true });
    await rm(socketDir, { recursive: true, force: true });
  }
});

test('the previous SDK is disposed and rejected before a native socket is opened', async () => {
  const root = await mkdtemp(join(tmpdir(), 'r67-legacy-sdk-'));
  const socketDir = await mkdtemp(join(tmpdir(), 'r67-legacy-socket-'));
  const disposers = [];
  try {
    const dataDir = join(root, 'data'); await mkdir(dataDir, { mode: 0o700 });
    const bindingFile = join(root, 'binding.json');
    await writeFile(bindingFile, JSON.stringify({ version: 1, kind: 'clarification', threadId: 'thread',
      goalId: 'goal', goal: 'Goal', decision: 'Question?', expectedEventSha256: 'a'.repeat(64),
      senderId: '1', chatId: '2', capability: 'b'.repeat(64), socketDirectory: socketDir }), { mode: 0o600 });
    const ctx = { plugin: { dataDir }, settings: { get: () => bindingFile },
      ownerBridge: { bind: bindLegacyOwnerBridge }, onDispose: fn => disposers.push(fn) };
    await assert.rejects(activate(ctx), /routing guard/);
    await assert.rejects(lstat(join(socketDir, 'owner-bridge.sock')), { code: 'ENOENT' });
    // The incompatible SDK must not retain a binding that could consume later traffic.
    const rebound = bindLegacyOwnerBridge({ senderId: '1', chatId: '2', async onReply() { return false; } });
    rebound.dispose();
  } finally {
    for (const dispose of disposers.reverse()) await dispose();
    await rm(root, { recursive: true, force: true });
    await rm(socketDir, { recursive: true, force: true });
  }
});

test('a selected socket directory must be absolute, private and not a symlink', async () => {
  const root = await mkdtemp(join(tmpdir(), 'r67-socket-mode-'));
  try {
    const link = join(root, 'link');
    await symlink(root, link);
    await assert.rejects(privateSocketDirectory(root, 'relative'), /absolute/);
    await assert.rejects(privateSocketDirectory(root, link), /unsafe/);
    await chmod(root, 0o755);
    await assert.rejects(privateSocketDirectory(root, root), /unsafe/);
    assert.equal((await lstat(root)).mode & 0o777, 0o755);
  } finally { await rm(root, { recursive: true, force: true }); }
});
