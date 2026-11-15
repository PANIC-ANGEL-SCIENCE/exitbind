// Mercury's opt-in channel boundary. It never starts or replaces a poller.
import { randomBytes } from 'node:crypto';
export type OwnerBridgeRoutingReason = 'matched_reply' | 'missing_reply_to' | 'other_reply_to';
export interface OwnerReply {
  senderId: string;
  chatId: string;
  messageId: string;
  replyToMessageId: string;
  text: string;
}
export interface OwnerMessage extends Omit<OwnerReply, 'replyToMessageId'> {
  replyToMessageId?: string;
  replyToIsBridgeQuestion: boolean;
}

export interface OwnerBridgeOptions {
  senderId: string;
  chatId: string;
  messageId?: string;
  sendUncertain?: boolean;
  isQuestionActive(): boolean;
  onRouting(reason: OwnerBridgeRoutingReason): Promise<void>;
  onReply(reply: OwnerReply): Promise<boolean>;
}

export interface OwnerBridgeHost {
  isOwner(senderId: string, chatId: string): boolean;
  send(senderId: string, chatId: string, text: string, forceReply: boolean): Promise<string>;
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

export function bindOwnerBridge(options: OwnerBridgeOptions): { readonly routingGuard: 1; sendQuestion(text: string): Promise<string>; dispose(): void } {
  if (active) throw new Error('owner bridge already bound');
  if (!options.senderId || !options.chatId || typeof options.onReply !== 'function' ||
      typeof options.isQuestionActive !== 'function' || typeof options.onRouting !== 'function') {
    throw new Error('invalid owner bridge binding');
  }
  const binding: Active = { ...options, disposed: false };
  active = binding;
  return {
    routingGuard: 1,
    async sendQuestion(text: string): Promise<string> {
      const current = host;
      if (binding.disposed || !current || !current.isOwner(binding.senderId, binding.chatId) ||
          !text || text.length > 3900 || binding.pending || binding.messageId) {
        throw new Error('owner bridge cannot send');
      }
      const marker = randomBytes(16).toString('hex');
      const message = `[Exitbind question ${marker}]\n${text}\n\nPlease reply directly to this message.`;
      const pending = Promise.resolve().then(() => current.send(binding.senderId, binding.chatId, message, true));
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

// Reason codes contain no message content or identifiers. A failed observation
// cannot let a rejected owner message fall through into ordinary agent work.
async function observe(binding: Active, reason: OwnerBridgeRoutingReason): Promise<void> {
  try { await binding.onRouting(reason); } catch { /* keep the routing decision */ }
}

function isQuestionActive(binding: Active): boolean {
  try { return binding.isQuestionActive() === true; }
  catch { return true; } // An unavailable state must not route an answer to the agent.
}

// Authenticated selected owner/chat only. Messages without a matching reply-to
// are rejected, never interpreted as an answer or passed to ordinary chat.
export async function routeOwnerMessage(message: OwnerMessage): Promise<boolean> {
  const binding = active;
  const current = host;
  if (!binding || binding.disposed || !current || !current.isOwner(message.senderId, message.chatId) ||
      binding.senderId !== message.senderId || binding.chatId !== message.chatId) return false;
  const activeAtArrival = isQuestionActive(binding);
  if (binding.pending && message.replyToMessageId) {
    try { binding.messageId = await binding.pending; } catch { /* send outcome remains uncertain */ }
  }
  if (active !== binding || binding.disposed || host !== current ||
      !current.isOwner(message.senderId, message.chatId)) return activeAtArrival || message.replyToIsBridgeQuestion;
  if (message.replyToMessageId && message.replyToMessageId === binding.messageId) {
    if (activeAtArrival) await observe(binding, 'matched_reply');
    try { await binding.onReply({ senderId: message.senderId, chatId: message.chatId,
      messageId: message.messageId, replyToMessageId: message.replyToMessageId, text: message.text }); }
    catch { /* the bound reply remains consumed */ }
    return true;
  }
  // An uncertain send only quarantines replies that identify a bridge question.
  if (binding.sendUncertain && message.replyToIsBridgeQuestion) return true;
  if (!activeAtArrival) return false;
  await observe(binding, message.replyToMessageId ? 'other_reply_to' : 'missing_reply_to');
  if (active === binding && !binding.disposed && host === current &&
      current.isOwner(binding.senderId, binding.chatId) && isQuestionActive(binding)) {
    try {
      await current.send(binding.senderId, binding.chatId,
        'Please reply directly to the question message. This message was not submitted.', false);
    } catch { /* a failed guide must not become a chat turn or be replayed */ }
  }
  return true;
}
