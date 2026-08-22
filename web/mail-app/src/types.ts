export interface RuntimeConfig {
  apiBaseUrl: string;
  /** Optional deployment preference. The authenticated directory remains authoritative. */
  mailboxId?: string;
}
export interface MailboxScopedConfig extends RuntimeConfig {
  mailboxId: string;
}
export type MailboxKind = "human" | "agent" | "shared" | "group";
export interface MailboxDirectoryItem {
  mailbox_id: string;
  kind: MailboxKind;
  primary_address: string;
  can_send: boolean;
}
export interface MailboxDirectoryPage {
  items: MailboxDirectoryItem[];
  next_cursor: string | null;
  has_more: boolean;
}
export interface HumanSessionResponse {
  principal_id: string;
  organization_id: string;
  client: "admin" | "mail";
  expires_at: string;
}
export interface MessageSummary {
  message_id: string;
  subject: string;
  from: { name: string | null; email: string }[];
  received_at: string;
  preview: string;
  is_read: boolean;
  is_flagged: boolean;
  has_attachment: boolean;
}
export interface InboxPage {
  items: MessageSummary[];
  next_cursor: string | null;
  has_more: boolean;
}
export interface MessageDetail {
  message: MessageSummary;
  reply_to_message_id: string | null;
  to: { name: string | null; email: string }[];
  cc: { name: string | null; email: string }[];
  text_body: string;
  text_body_truncated: boolean;
  attachments: { name: string | null; media_type: string; size_bytes: number }[];
}
export interface MessageState {
  message_id: string;
  is_read: boolean;
  is_flagged: boolean;
}
export interface DraftInput {
  draft_id: string | null;
  expected_version: number | null;
  reply_to_message_id: string | null;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  text_body: string;
}
export interface DraftResult {
  draft_id: string;
  version: number;
  reply_to_message_id: string | null;
  previous_draft_destroyed: boolean;
}
export interface HumanDraftListItem {
  draft_id: string;
  version: number;
  reply_to_message_id: string | null;
  recipient_count: number;
  created_at: string;
  updated_at: string;
}
export interface HumanDraftPage {
  items: HumanDraftListItem[];
  next_cursor: string | null;
  has_more: boolean;
}
export interface HumanDraftRestore {
  draft_id: string;
  version: number;
  reply_to_message_id: string | null;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  text_body: string;
}
export interface MailSubmission {
  submission_id: string;
  mailbox_id: string;
  draft_id: string;
  draft_version: number;
  status: "pending" | "reconciling" | "accepted" | "rejected" | "manual_review";
  state_version: number;
  created_at: string;
  updated_at: string;
  delivery: MailDelivery | null;
}
export interface MailDeliveryRecipient {
  recipient_ref: string;
  status: "queued" | "delivered" | "permanent_failure" | "unknown";
  smtp_reply: string;
}
export interface MailDelivery {
  observed_at: string;
  terminal: boolean;
  recipient_count: number;
  delivered_count: number;
  failed_count: number;
  queued_count: number;
  unknown_count: number;
  recipients: MailDeliveryRecipient[];
}
