import { describe, expect, it } from "vitest";
import {
  MessageDetailRequestGate,
  detailMatchesSelection,
} from "./detail-request";
import type { MessageDetail } from "./types";

const messageA = "018f0000-0000-7000-8000-000000000009";
const messageB = "018f0000-0000-7000-8000-000000000010";

function detail(messageId: string): MessageDetail {
  return {
    message: {
      message_id: messageId,
      subject: "Status",
      from: [{ name: null, email: "sender@example.test" }],
      received_at: "2026-07-27T00:00:00Z",
      preview: "Ready.",
      is_read: false,
      is_flagged: false,
      has_attachment: false,
    },
    reply_to_message_id: messageId,
    to: [],
    cc: [],
    text_body: "Body",
    text_body_truncated: false,
    attachments: [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("MessageDetailRequestGate", () => {
  it("accepts only the latest deferred A-to-B detail result", async () => {
    const gate = new MessageDetailRequestGate();
    const firstToken = gate.begin(messageA)!;
    const first = deferred<MessageDetail>();
    const accepted: string[] = [];
    const firstHandled = first.promise.then((value) => {
      if (gate.accepts(firstToken, value)) accepted.push(value.message.message_id);
    });

    gate.invalidate();
    const secondToken = gate.begin(messageB)!;
    const second = deferred<MessageDetail>();
    const secondHandled = second.promise.then((value) => {
      if (gate.accepts(secondToken, value)) accepted.push(value.message.message_id);
    });

    second.resolve(detail(messageB));
    await secondHandled;
    first.resolve(detail(messageA));
    await firstHandled;

    expect(accepted).toEqual([messageB]);
  });

  it("does not issue another request token for a state-updated summary with the same UUID", () => {
    const gate = new MessageDetailRequestGate();

    expect(gate.begin(messageA)).toBeDefined();
    expect(gate.begin(messageA)).toBeUndefined();
  });

  it("requires both displayed and reply identities to match the selection", () => {
    expect(detailMatchesSelection(detail(messageA), messageA)).toBe(true);
    expect(detailMatchesSelection(detail(messageA), messageB)).toBe(false);
    expect(detailMatchesSelection({
      ...detail(messageA),
      reply_to_message_id: messageB,
    }, messageA)).toBe(false);
  });
});
