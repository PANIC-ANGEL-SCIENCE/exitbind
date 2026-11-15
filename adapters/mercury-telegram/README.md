# Optional Mercury/Telegram owner bridge

This source package connects one explicitly bound clarification to an existing
Codex client and one paired Mercury Telegram owner. The client that already
owns the live app-server connection can forward a server request and write the
response to that same connection. An explicitly configured MCP server can also
accept a tool call from the original Codex thread. The bridge does not start a
Codex thread, resume stored history, launch a model, poll Telegram, or interpret
an answer as permission. Its app-server path handles a single question with
`autoResolutionMs` set to `null`; other requests are left to the existing client.
The caller must select a clarification, not an approval prompt, and keep
dependent work waiting even when Codex marks the question nonblocking.

The package is optional source in this repository. It is not included in the
current Cargo source archive or installed binary. Node.js 22.18 or newer is
needed for its tests and host integration. `mercury-sdk.patch` is an opt-in
extension against Mercury source commit
`62135a04d511995ac6c17af44d602eb6b1fc4b2a` (Mercury 1.5.2). It adds a declared
`telegramOwnerBridge` plugin permission, a narrow SDK boundary, and Telegram
delivery/response hooks. It preserves the ordinary consumer for unrelated
messages. Apply and build that source only in an explicitly selected Mercury
installation; having this patch does not change an already running Mercury.
Deploy the SDK extension and adapter together. The adapter refuses an older SDK
before opening its socket, and the updated SDK refuses an older adapter that
does not supply the required routing callbacks.

The extension checks the selected sender and private chat against Mercury's
current approved admins before sending or accepting a response. It sends one
plain-text question with a direct-reply instruction and Telegram's ForceReply
prompt, and returns Telegram's actual message ID. Replies must reference that
message, and their text is returned without trimming or model rewriting. While
the bound question is active, other text from that selected owner and private
chat is rejected with one short instruction to reply to the question. It is not
submitted, stored, queued, or replayed. Other senders and chats continue through
Mercury's original routing. The bot-authored question has a random marker so a
late reply to it is consumed even while the plugin is stopped. If the send
outcome is unknown, only replies identifying a marked bridge question are
held out of ordinary chat; other replies follow the usual route.

## Prepare one binding

The original Lead first records the exact question as an open Goal decision and
retains the resulting `eventSha256`. The owner selects one already paired
Telegram admin and private chat. In a private `0600` JSON file, supply these
fields using the real values for that request:

```json
{
  "version": 1,
  "kind": "clarification",
  "threadId": "selected-live-thread",
  "goalId": "selected-goal",
  "goal": "The exact current Goal text",
  "decision": "The exact open question?",
  "expectedEventSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "senderId": "12345",
  "chatId": "12345",
  "capability": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
}
```

The example hash, IDs, and capability are placeholders, not valid bindings.
Generate a fresh unpredictable 32-byte capability for each real binding; keep it
and the filled file private. A thread ID locates a native session and does not
authenticate a person. Point the plugin's `bindingFile` setting at this file.
Mercury's plugin data directory must be owned by the current user and must not
be writable by other users. The bridge creates its own private (`0700`) child
directory, and refuses an unsafe existing child or state file. No permissions
on existing directories or credentials are changed by this source package.
The resulting socket path must fit within 100 UTF-8 bytes; the adapter refuses
longer paths before listening. A host with a longer Mercury data path needs a
shorter location: first select or create an existing directory owned by the
current user with mode `0700`, then add its absolute path as the optional
`"socketDirectory"` field in the private binding file. The bridge validates
that directory and does not create it or change its permissions. Private state
continues to live in the plugin data directory; the selected directory holds
only the socket.

An installed plugin receives the new `ctx.ownerBridge` capability only if its
manifest declares `telegramOwnerBridge`. The capability is scoped to a single
configured sender, chat, and question message. Its local state retains the
minimal correlation and exact answer in the private plugin data directory. A
single-writer lock prevents two plugin instances from sending the same request.
While the selected question is active, fixed routing-reason counters record
whether replies matched, lacked a reply target, or targeted another message.
Each counter saturates at 255; these observations contain no message text or
incoming message identifiers.
An existing lock or socket is not removed automatically: determine whether its
owner is still active before selecting recovery.

## Integrate through MCP

`mcp.mjs` is a stdio MCP server that can be configured in the existing Codex
client. Start it with Node and the path to an owner-only (`0600`) JSON settings
file containing `version: 1` and the absolute paths `bindingFile`, `socketPath`,
`exitbindCommand`, and `projectConfig`. The binding and socket are the same
ones used by the Mercury plugin. The Exitbind command reads the explicitly
selected project Goal; the MCP server does not write a Goal or adopt another
session's identity.

The `ask_owner` tool accepts only the exact bound question and the original
thread ID in the MCP call's `_meta.threadId`. It returns the owner's exact text
to the same MCP request. The client must actually load this server before the
tool exists; a configuration file alone does not establish that a running
client reloaded it. `report_observation` can forward the Lead's separate
native and guarded Goal observations. Its reply confirms only forwarding;
inspect the private bridge state to confirm what was recorded.

Codex MCP tool calls have a configured finite timeout; a timeout or cancellation
ends that request without an answer and leaves the Goal decision open. This
route does not establish an indefinitely pending native call, and it must not
replay an answer or infer a default when a request has ended.

## Integrate the live app-server client

`native-client.mjs` exports `connectNative`. Supply the plugin's private
`owner-bridge-private/owner-bridge.sock` path within the plugin data directory
(or `owner-bridge.sock` in the selected `socketDirectory`),
the capability, the same binding, a `sendResponse`
callback that writes `{ id, result }` to the original app-server connection,
an `isCurrent` callback that reads the Goal through the original owner's context,
and an `onUnavailable` callback. The callback must confirm the exact bound Goal
event and open decision; unavailable is not current. The client checks it before
forwarding the request and again before writing the answer. These reads and the
native write are separate operations, so a concurrent Goal change can still
occur between them. After connection, the existing client calls
`forward(message)` for each server request. A `true` result means that the
matching clarification is now owned by this bridge; it does not mean the
question or answer was delivered. A `false` result leaves the request with the
original client. Forward `serverRequest/resolved` notifications with
`resolved(message)` and close the bridge when the native client disconnects.
The client must not answer the same forwarded request through another route.

After separately observing the native outcome, the trusted client may call
`reportNative` with the thread ID, original request ID, `outcome: "accepted"`,
the SHA-256 of the exact answer, and an evidence reference. After the original
Lead's guarded Goal command, it may call `reportGoal` with the returned Goal
record and result reference. The adapter checks these against its binding and
records them separately as *reported* observations, not independent proof.
When both have been reported it drops the stored plaintext answer and retains
the answer hash and references. If either observation is missing or uncertain,
the answer is not replayed and the unresolved state remains available for
inspection.

The socket is restricted by the private directory, socket mode, and a capability.
It is not a security boundary against another process with the same account's
authority. Keep the binding out of logs and tracked files. Only an already
attached, trusted native client can supply the request; the bridge never
discovers or resumes a thread. A standard client must explicitly integrate this
shim before it can be used. An installed Codex proxy and a live thread ID alone
do not establish that integration or a same-thread round trip.

When an answer arrives, the adapter records the attempt before writing the
response. A successful socket write or `serverRequest/resolved` is not proof of
native acceptance: that notification is also emitted on cleanup. Confirm the
actual response and continuation in the original client. The original Lead then
uses its own native Goal context to record the answer with the same question,
an exact result reference, `--disposition accepted`, and
`--expected-event-sha256` set to the retained Goal event. The guard checks the
current Goal and open question under the Goal journal lock. If it refuses,
inspect the changed Goal; do not replay the answer into another decision. The
adapter never constructs a parent session identity or writes the Goal itself.

Send, native response, and Goal recording are separate effects. A failed or
lost acknowledgement can leave an effect unknown; inspect the original client,
message, and Goal before recovery. No answer, timeout, or default is treated as
an owner decision. A disconnect or process restart requires explicit
revalidation, and a saved message cannot revive a dead native request. The
current single-binding implementation does not automatically rearm for another
question.

Run the local fixture suite with `node --test test/*.test.mjs` from this
directory. It exercises the adapter and its private local connection, not a
real Mercury delivery or the current Lead's native continuation.
