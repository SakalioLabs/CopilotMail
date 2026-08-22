import type { MessageDetail } from "./types";

export interface MessageDetailRequestToken {
  readonly generation: number;
  readonly messageId: string;
}

export function detailMatchesSelection(
  detail: MessageDetail | undefined,
  selectedMessageId: string | undefined,
): detail is MessageDetail {
  return detail !== undefined
    && selectedMessageId !== undefined
    && detail.message.message_id === selectedMessageId
    && (
      detail.reply_to_message_id === null
      || detail.reply_to_message_id === selectedMessageId
    );
}

/**
 * Keeps asynchronous detail results bound to the latest selected safe UUID.
 * Re-selecting an updated summary for the same UUID deliberately creates no
 * new token, so read/flag state updates cannot trigger another content read.
 */
export class MessageDetailRequestGate {
  private generation = 0;
  private selectedMessageId: string | undefined;

  begin(messageId: string): MessageDetailRequestToken | undefined {
    if (this.selectedMessageId === messageId) return undefined;
    this.selectedMessageId = messageId;
    this.generation += 1;
    return { generation: this.generation, messageId };
  }

  invalidate(): void {
    this.selectedMessageId = undefined;
    this.generation += 1;
  }

  isCurrent(token: MessageDetailRequestToken): boolean {
    return token.generation === this.generation
      && token.messageId === this.selectedMessageId;
  }

  accepts(token: MessageDetailRequestToken, detail: MessageDetail): boolean {
    return this.isCurrent(token)
      && detailMatchesSelection(detail, token.messageId);
  }
}
