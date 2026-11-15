// Mercury's opt-in channel boundary. It never starts or replaces a poller.
import { randomBytes } from 'node:crypto';
export interface OwnerReply {
  senderId: string;
  chatId: string;
  messageId: string;
  replyToMessageId: string;
  text: string;
}

export interface OwnerBridgeOptions {
  senderId: string;
  chatId: string;
  messageId?: string;
  sendUncertain?: boolean;
  onReply(reply: OwnerReply): Promise<boolean>;
}

export interface OwnerBridgeHost {
  isOwner(senderId: string, chatId: string): boolean;
  send(senderId: string, chatId: string, text: string): Promise<string>;
}

interface Active extends OwnerBridgeOptions {
  pending?: Promise<string>;
  disposed: boolean;
}

let host: OwnerBridgeHost | undefined;
let active: Active | undefined;

export function isOwnerBridgeQuestion(fromBot: boolean, text: string | undefined): boolean {
  return fromBot && typeof text === 'string' && /^\[Exitbind question [a-f0-9]{32}\]\n/.test(text);
}

export function setOwnerBridgeHost(value: OwnerBridgeHost): void { host = value; }
export function clearOwnerBridgeHost(value: OwnerBridgeHost): void { if (host === value) host = undefined; }

export function bindOwnerBridge(options: OwnerBridgeOptions): { sendQuestion(text: string): Promise<string>; dispose(): void } {
  if (active) throw new Error('owner bridge already bound');
  if (!options.senderId || !options.chatId || typeof options.onReply !== 'function') {
    throw new Error('invalid owner bridge binding');
  }
  const binding: Active = { ...options, disposed: false };
  active = binding;
  return {
    async sendQuestion(text: string): Promise<string> {
      const current = host;
      if (binding.disposed || !current || !current.isOwner(binding.senderId, binding.chatId) ||
          !text || text.length > 3900 || binding.pending || binding.messageId) {
        throw new Error('owner bridge cannot send');
      }
      const marker = randomBytes(16).toString('hex');
      const pending = Promise.resolve().then(() => current.send(binding.senderId, binding.chatId, `[Exitbind question ${marker}]\n${text}`));
      binding.pending = pending;
      try {
        const id = await pending;
        if (!id || binding.disposed) throw new Error('owner bridge send outcome is unknown');
        binding.messageId = id;
        return id;
      } catch (error) {
        binding.sendUncertain = true;
        throw error;
      } finally { binding.pending = undefined; }
    },
    dispose(): void { binding.disposed = true; if (active === binding) active = undefined; }
  };
}

// Returns true only for this bridge's replies, including duplicates and stale
// replies, so they cannot fall through to Mercury's ordinary message handler.
export async function routeOwnerReply(reply: OwnerReply): Promise<boolean> {
  const binding = active;
  const current = host;
  if (!binding || binding.disposed || !current || !current.isOwner(reply.senderId, reply.chatId) ||
      binding.senderId !== reply.senderId || binding.chatId !== reply.chatId || !reply.replyToMessageId) return false;
  if (binding.pending) {
    try { binding.messageId = await binding.pending; }
    catch { return true; } // Uncertain send: do not turn a possible answer into a Mercury chat turn.
  }
  if (binding.sendUncertain) return true;
  if (active !== binding || binding.disposed || host !== current ||
      !current.isOwner(reply.senderId, reply.chatId) || reply.replyToMessageId !== binding.messageId) return false;
  try { await binding.onReply(reply); } catch { /* the bound reply remains consumed */ }
  return true;
}
