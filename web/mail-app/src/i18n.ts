export const SUPPORTED_LOCALES = ["zh-CN", "en"] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

export const DEFAULT_LOCALE: Locale = "zh-CN";
export const LOCALE_STORAGE_KEY = "copilotmail.mail.locale.v1";
export const TRANSLATION_FALLBACK = "—";

const zhCN = {
  "app.name": "CopilotMail",
  "common.emptyValue": "—",
  "common.recipientSeparator": "、",
  "icon.compose": "＋",
  "icon.refresh": "↻",
  "icon.search": "⌕",
  "icon.close": "×",
  "icon.starred": "★",
  "icon.notStarred": "☆",
  "icon.attachment": " 📎",
  "icon.emptyDetail": "✉",
  "icon.signOut": "↪",
  "sender.unknown": "未知发件人",
  "subject.none": "（无主题）",
  "auth.checking": "正在检查企业会话…",
  "auth.tagline": "你的工作邮件，清晰而安静。",
  "auth.signIn": "使用企业账号登录",
  "nav.mailboxLabel": "邮箱",
  "nav.mobileActions": "邮件操作",
  "nav.mobileViews": "邮件视图",
  "nav.compose": "写信",
  "nav.inbox": "收件箱",
  "nav.sent": "已发送",
  "nav.drafts": "草稿",
  "nav.signOut": "退出登录",
  "mailbox.selectorLabel": "当前邮箱",
  "mailbox.selectorAria": "切换邮箱",
  "mailbox.option": "{address} · {kind}",
  "mailbox.kind.human": "个人",
  "mailbox.kind.agent": "Agent",
  "mailbox.kind.shared": "共享",
  "mailbox.kind.group": "群组",
  "mailbox.readOnly": "此邮箱为只读",
  "mailbox.composeUnavailable": "你没有从此邮箱发送邮件的权限",
  "mailbox.loadingMore": "正在加载更多邮箱…",
  "mailbox.loadMore": "加载更多邮箱",
  "mailbox.directoryTitle": "你的企业邮箱",
  "mailbox.directoryLoading": "正在读取可访问的邮箱…",
  "mailbox.directoryNeedsMore": "当前页没有可用邮箱，请加载下一页。",
  "mailbox.retryDirectory": "重试读取",
  "inbox.searchResults": "搜索结果",
  "inbox.syncing": "正在同步…",
  "inbox.messageCount.one": "{count} 封邮件",
  "inbox.messageCount.other": "{count} 封邮件",
  "inbox.refreshAria": "刷新",
  "inbox.searchAria": "搜索邮件",
  "inbox.searchPlaceholder": "搜索发件人、主题或正文",
  "inbox.clearSearchAria": "清除搜索",
  "inbox.submitSearchAria": "搜索",
  "inbox.noSearchResults": "没有匹配的邮件",
  "inbox.empty": "收件箱是空的",
  "inbox.loadingMore": "正在加载…",
  "inbox.loadMore": "加载更多",
  "drafts.heading": "草稿",
  "drafts.syncing": "正在同步…",
  "drafts.count.one": "{count} 个草稿",
  "drafts.count.other": "{count} 个草稿",
  "drafts.refreshAria": "刷新草稿",
  "drafts.empty": "此邮箱没有可恢复的草稿",
  "drafts.newDraft": "新邮件草稿",
  "drafts.replyDraft": "回复草稿",
  "drafts.recipientCount.one": "{count} 位收件人",
  "drafts.recipientCount.other": "{count} 位收件人",
  "drafts.restore": "恢复编辑",
  "drafts.restoring": "正在恢复…",
  "drafts.loadMore": "加载更多草稿",
  "drafts.loadingMore": "正在加载…",
  "drafts.detailHint": "选择一个草稿以恢复编辑",
  "drafts.confirmTitle": "替换当前邮件？",
  "drafts.confirmDescription": "恢复此草稿将丢弃当前写信窗口中的内容。",
  "drafts.confirmCancel": "保留当前内容",
  "drafts.confirmAction": "恢复草稿",
  "sent.heading": "已发送",
  "sent.syncing": "正在同步…",
  "sent.count.one": "正在显示 {count} 封已发送邮件",
  "sent.count.other": "正在显示 {count} 封已发送邮件",
  "sent.refreshAria": "刷新已发送邮件",
  "sent.empty": "此邮箱还没有已接受发送的邮件",
  "sent.accepted": "已接受发送",
  "sent.recipientCount.one": "{count} 位收件人",
  "sent.recipientCount.other": "{count} 位收件人",
  "sent.awaitingDelivery": "已接受发送，正在等待投递状态更新",
  "sent.deliveryDelivered": "已投递至全部 {count} 个收件方服务器",
  "sent.deliveryCompleted": "投递完成：成功 {delivered}，失败 {failed}",
  "sent.deliveryProgress": "投递更新：成功 {delivered}/{total}，失败 {failed}，排队 {queued}，未知 {unknown}",
  "sent.deliveryUpdated": "更新于 {date}",
  "sent.loadMore": "加载更多已发送邮件",
  "sent.loadingMore": "正在加载…",
  "sent.detailHint": "已发送目录仅显示投递摘要，不显示邮件正文或收件人明细",
  "message.hasAttachment": "有附件",
  "message.starred": "已加星标",
  "message.unread": "未读",
  "detail.eyebrow": "邮件",
  "detail.backToInbox": "返回收件箱",
  "detail.removeStarAria": "取消星标",
  "detail.addStarAria": "添加星标",
  "detail.loading": "正在读取…",
  "detail.retry": "重试读取",
  "detail.recipients": "收件人：{recipients}",
  "detail.noSafeText": "此邮件没有可安全显示的纯文本正文。",
  "detail.reply": "回复",
  "detail.bodyTruncated": "正文过长，已安全截断。",
  "detail.attachments": "附件",
  "detail.unnamedAttachment": "未命名附件",
  "detail.attachmentMeta": "{mediaType} · {size}",
  "detail.selectMessage": "选择一封邮件查看",
  "compose.newTitle": "新邮件",
  "compose.replyTitle": "回复邮件",
  "compose.editDraftTitle": "编辑草稿",
  "compose.closeAria": "关闭写信窗口",
  "compose.replyContext": "回复关系由服务器验证并绑定到原邮件",
  "compose.from": "发件人",
  "compose.to": "收件人",
  "compose.toPlaceholder": "name@example.com；多个地址用分号分隔",
  "compose.ccBcc": "抄送 / 密送",
  "compose.cc": "抄送",
  "compose.bcc": "密送",
  "compose.subject": "主题",
  "compose.body": "正文",
  "compose.bodyPlaceholder": "写下邮件内容…",
  "compose.send": "发送",
  "compose.sending": "发送中…",
  "compose.retrySend": "重试发送",
  "compose.replySubject": "Re: {subject}",
  "draft.autosaveHint": "输入收件人后自动保存",
  "draft.saving": "正在保存…",
  "draft.saved": "草稿已保存",
  "draft.savedCleanupFailed": "草稿已保存；旧版本清理失败，请勿重复编辑",
  "draft.waitingForAutosave": "等待自动保存…",
  "draft.autosaveFailed": "自动保存失败，将在下次编辑时重试",
  "draft.replyAutosaveHint": "回复将绑定到原邮件；输入正文后自动保存",
  "draft.restored": "草稿已恢复；后续修改将自动保存",
  "delivery.allDelivered": "已投递至全部 {count} 个收件方服务器",
  "delivery.completed": "投递完成：成功 {delivered}，失败 {failed}",
  "delivery.acceptedProgress": "邮件服务器已接受；已投递 {delivered}/{total}",
  "delivery.acceptedPending": "邮件服务器已接受，正在获取最终投递状态…",
  "delivery.rejected": "邮件服务器拒绝了发送；请联系管理员",
  "delivery.manualReview": "发送状态需要人工检查，请勿重复发送",
  "delivery.queued": "已进入发送队列…",
  "delivery.confirming": "正在确认发送结果…",
  "delivery.stillProcessing": "发送仍在后台处理，可稍后继续查看",
  "delivery.requestUnknown": "发送请求状态未知；请重试，系统不会重复发送",
  "delivery.draftSaveFailed": "发送前无法保存草稿，请检查内容后重试",
  "error.loginFailed": "无法开始登录，请重试",
  "error.sessionUnavailable": "暂时无法验证企业会话，请重试",
  "error.signOutFailed": "退出登录失败，请重试",
  "error.mailboxDirectoryUnavailable": "暂时无法验证你可访问的邮箱",
  "error.mailboxDirectoryEmpty": "你的账号当前没有可访问的邮箱，请联系管理员",
  "error.mailboxDirectoryPageUnavailable": "无法继续加载邮箱目录，请重试",
  "error.inboxUnavailable": "暂时无法读取收件箱",
  "error.messageUnavailable": "暂时无法读取邮件",
  "error.updateReadState": "无法更新已读状态",
  "error.searchUnavailable": "暂时无法搜索邮件",
  "error.searchTooShort": "请至少输入两个字符",
  "error.loadMoreSearch": "无法继续加载搜索结果",
  "error.loadMoreInbox": "无法继续加载收件箱",
  "error.updateStar": "无法更新星标",
  "error.draftsUnavailable": "暂时无法读取草稿",
  "error.draftsPageUnavailable": "无法继续加载草稿，请重试",
  "error.draftRestoreUnavailable": "暂时无法恢复此草稿",
  "error.draftRestoreConflict": "此草稿已在其他位置变化；列表已刷新",
  "error.draftSubmissionLocked": "当前邮件的发送状态尚未确定，不能打开其他草稿",
  "error.sentUnavailable": "暂时无法读取已发送邮件",
  "error.sentPageUnavailable": "无法继续加载已发送邮件，请重试",
  "locale.label": "语言",
  "locale.zhCN": "简体中文",
  "locale.en": "English",
} as const;

export type MessageKey = keyof typeof zhCN;

const en = {
  "app.name": "CopilotMail",
  "common.emptyValue": "—",
  "common.recipientSeparator": ", ",
  "icon.compose": "＋",
  "icon.refresh": "↻",
  "icon.search": "⌕",
  "icon.close": "×",
  "icon.starred": "★",
  "icon.notStarred": "☆",
  "icon.attachment": " 📎",
  "icon.emptyDetail": "✉",
  "icon.signOut": "↪",
  "sender.unknown": "Unknown sender",
  "subject.none": "(No subject)",
  "auth.checking": "Checking your organization session…",
  "auth.tagline": "Your work email, clear and calm.",
  "auth.signIn": "Sign in with your organization account",
  "nav.mailboxLabel": "Mailbox",
  "nav.mobileActions": "Mail actions",
  "nav.mobileViews": "Mail views",
  "nav.compose": "Compose",
  "nav.inbox": "Inbox",
  "nav.sent": "Sent",
  "nav.drafts": "Drafts",
  "nav.signOut": "Sign out",
  "mailbox.selectorLabel": "Current mailbox",
  "mailbox.selectorAria": "Switch mailbox",
  "mailbox.option": "{address} · {kind}",
  "mailbox.kind.human": "Personal",
  "mailbox.kind.agent": "Agent",
  "mailbox.kind.shared": "Shared",
  "mailbox.kind.group": "Group",
  "mailbox.readOnly": "This mailbox is read-only",
  "mailbox.composeUnavailable": "You do not have permission to send from this mailbox",
  "mailbox.loadingMore": "Loading more mailboxes…",
  "mailbox.loadMore": "Load more mailboxes",
  "mailbox.directoryTitle": "Your organization mailboxes",
  "mailbox.directoryLoading": "Loading the mailboxes you can access…",
  "mailbox.directoryNeedsMore": "No mailbox is available on this page. Load the next page.",
  "mailbox.retryDirectory": "Retry",
  "inbox.searchResults": "Search results",
  "inbox.syncing": "Syncing…",
  "inbox.messageCount.one": "{count} message",
  "inbox.messageCount.other": "{count} messages",
  "inbox.refreshAria": "Refresh",
  "inbox.searchAria": "Search email",
  "inbox.searchPlaceholder": "Search sender, subject, or message body",
  "inbox.clearSearchAria": "Clear search",
  "inbox.submitSearchAria": "Search",
  "inbox.noSearchResults": "No matching messages",
  "inbox.empty": "Your inbox is empty",
  "inbox.loadingMore": "Loading…",
  "inbox.loadMore": "Load more",
  "drafts.heading": "Drafts",
  "drafts.syncing": "Syncing…",
  "drafts.count.one": "{count} draft",
  "drafts.count.other": "{count} drafts",
  "drafts.refreshAria": "Refresh drafts",
  "drafts.empty": "This mailbox has no draft that can be restored",
  "drafts.newDraft": "New message draft",
  "drafts.replyDraft": "Reply draft",
  "drafts.recipientCount.one": "{count} recipient",
  "drafts.recipientCount.other": "{count} recipients",
  "drafts.restore": "Resume editing",
  "drafts.restoring": "Restoring…",
  "drafts.loadMore": "Load more drafts",
  "drafts.loadingMore": "Loading…",
  "drafts.detailHint": "Select a draft to resume editing",
  "drafts.confirmTitle": "Replace the current message?",
  "drafts.confirmDescription": "Restoring this draft will discard the content in the current composer.",
  "drafts.confirmCancel": "Keep current content",
  "drafts.confirmAction": "Restore draft",
  "sent.heading": "Sent",
  "sent.syncing": "Syncing…",
  "sent.count.one": "Showing {count} sent message",
  "sent.count.other": "Showing {count} sent messages",
  "sent.refreshAria": "Refresh sent mail",
  "sent.empty": "This mailbox has no accepted sent messages yet",
  "sent.accepted": "Accepted for delivery",
  "sent.recipientCount.one": "{count} recipient",
  "sent.recipientCount.other": "{count} recipients",
  "sent.awaitingDelivery": "Accepted for delivery; waiting for a delivery status update",
  "sent.deliveryDelivered": "Delivered to all {count} recipient servers",
  "sent.deliveryCompleted": "Delivery complete: {delivered} succeeded, {failed} failed",
  "sent.deliveryProgress": "Delivery update: {delivered}/{total} delivered, {failed} failed, {queued} queued, {unknown} unknown",
  "sent.deliveryUpdated": "Updated {date}",
  "sent.loadMore": "Load more sent mail",
  "sent.loadingMore": "Loading…",
  "sent.detailHint": "The sent directory shows delivery summaries only, never message content or recipient details",
  "message.hasAttachment": "Has attachments",
  "message.starred": "Starred",
  "message.unread": "Unread",
  "detail.eyebrow": "Message",
  "detail.backToInbox": "Back to inbox",
  "detail.removeStarAria": "Remove star",
  "detail.addStarAria": "Add star",
  "detail.loading": "Loading…",
  "detail.retry": "Retry",
  "detail.recipients": "To: {recipients}",
  "detail.noSafeText": "This message has no plain-text body that can be displayed safely.",
  "detail.reply": "Reply",
  "detail.bodyTruncated": "The message body was safely truncated because it is too long.",
  "detail.attachments": "Attachments",
  "detail.unnamedAttachment": "Unnamed attachment",
  "detail.attachmentMeta": "{mediaType} · {size}",
  "detail.selectMessage": "Select a message to read",
  "compose.newTitle": "New message",
  "compose.replyTitle": "Reply",
  "compose.editDraftTitle": "Edit draft",
  "compose.closeAria": "Close composer",
  "compose.replyContext": "The server verifies and binds this reply to the original message",
  "compose.from": "From",
  "compose.to": "To",
  "compose.toPlaceholder": "name@example.com; separate multiple addresses with semicolons",
  "compose.ccBcc": "Cc / Bcc",
  "compose.cc": "Cc",
  "compose.bcc": "Bcc",
  "compose.subject": "Subject",
  "compose.body": "Message body",
  "compose.bodyPlaceholder": "Write your message…",
  "compose.send": "Send",
  "compose.sending": "Sending…",
  "compose.retrySend": "Retry send",
  "compose.replySubject": "Re: {subject}",
  "draft.autosaveHint": "Enter a recipient to enable autosave",
  "draft.saving": "Saving…",
  "draft.saved": "Draft saved",
  "draft.savedCleanupFailed": "Draft saved; the old version could not be removed, so avoid editing it again",
  "draft.waitingForAutosave": "Waiting to autosave…",
  "draft.autosaveFailed": "Autosave failed; it will retry after your next edit",
  "draft.replyAutosaveHint": "This reply will be linked to the original message and saved after you enter a body",
  "draft.restored": "Draft restored; further changes will be saved automatically",
  "delivery.allDelivered": "Delivered to all {count} recipient servers",
  "delivery.completed": "Delivery complete: {delivered} succeeded, {failed} failed",
  "delivery.acceptedProgress": "Accepted by the mail server; delivered {delivered}/{total}",
  "delivery.acceptedPending": "Accepted by the mail server; retrieving final delivery status…",
  "delivery.rejected": "The mail server rejected this message; contact an administrator",
  "delivery.manualReview": "The send status needs manual review; do not send it again",
  "delivery.queued": "Queued for sending…",
  "delivery.confirming": "Confirming the send result…",
  "delivery.stillProcessing": "Sending is still processing in the background; check again later",
  "delivery.requestUnknown": "The send request status is unknown; retrying will not send a duplicate",
  "delivery.draftSaveFailed": "The draft could not be saved before sending; review it and try again",
  "error.loginFailed": "Unable to start sign-in; try again",
  "error.sessionUnavailable": "Unable to verify your organization session; try again",
  "error.signOutFailed": "Unable to sign out; try again",
  "error.mailboxDirectoryUnavailable": "Unable to verify the mailboxes you can access",
  "error.mailboxDirectoryEmpty": "Your account has no accessible mailbox; contact an administrator",
  "error.mailboxDirectoryPageUnavailable": "Unable to load more mailboxes; try again",
  "error.inboxUnavailable": "The inbox is temporarily unavailable",
  "error.messageUnavailable": "This message is temporarily unavailable",
  "error.updateReadState": "Unable to update the read state",
  "error.searchUnavailable": "Search is temporarily unavailable",
  "error.searchTooShort": "Enter at least two characters",
  "error.loadMoreSearch": "Unable to load more search results",
  "error.loadMoreInbox": "Unable to load more inbox messages",
  "error.updateStar": "Unable to update the star",
  "error.draftsUnavailable": "Drafts are temporarily unavailable",
  "error.draftsPageUnavailable": "Unable to load more drafts; try again",
  "error.draftRestoreUnavailable": "This draft is temporarily unavailable",
  "error.draftRestoreConflict": "This draft changed elsewhere; the list was refreshed",
  "error.draftSubmissionLocked": "The current send status is not known, so another draft cannot be opened",
  "error.sentUnavailable": "Sent mail is temporarily unavailable",
  "error.sentPageUnavailable": "Unable to load more sent mail; try again",
  "locale.label": "Language",
  "locale.zhCN": "简体中文",
  "locale.en": "English",
} as const satisfies Record<MessageKey, string>;

export const MESSAGES = {
  "zh-CN": zhCN,
  en,
} as const;

type PlaceholderNames<Value extends string> =
  Value extends `${string}{${infer Parameter}}${infer Rest}`
    ? Parameter | PlaceholderNames<Rest>
    : never;

export type TranslationValue = string | number;
export type TranslationParams<Key extends MessageKey> = {
  [Parameter in PlaceholderNames<(typeof zhCN)[Key]>]: TranslationValue;
};
export type TranslationArgs<Key extends MessageKey> =
  [PlaceholderNames<(typeof zhCN)[Key]>] extends [never]
    ? []
    : [params: TranslationParams<Key>];
export type Translator = <Key extends MessageKey>(
  key: Key,
  ...args: TranslationArgs<Key>
) => string;

export interface LocaleStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface LocaleDetectionOptions {
  storage?: LocaleStorage | null;
  documentLanguage?: string | null;
  navigatorLanguages?: readonly string[] | null;
  fallback?: Locale;
}

const PARAMETER_PATTERN = /\{([A-Za-z][A-Za-z0-9_]*)\}/g;

function normalizeLocale(value: unknown): Locale | undefined {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim().replace(/_/g, "-").toLowerCase();
  if (normalized === "en" || normalized.startsWith("en-")) return "en";
  if (normalized === "zh" || normalized.startsWith("zh-")) return "zh-CN";
  return undefined;
}

function defaultStorage(): LocaleStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function browserDocumentLanguage(): string | null {
  try {
    return globalThis.document?.documentElement?.lang || null;
  } catch {
    return null;
  }
}

function browserNavigatorLanguages(): readonly string[] {
  try {
    const languages = Array.from(globalThis.navigator?.languages ?? []);
    const primary = globalThis.navigator?.language;
    return primary && !languages.includes(primary) ? [...languages, primary] : languages;
  } catch {
    return [];
  }
}

function selectedStorage(storage: LocaleStorage | null | undefined): LocaleStorage | null {
  return storage === undefined ? defaultStorage() : storage;
}

export function readLocale(storage?: LocaleStorage | null): Locale | undefined {
  const target = selectedStorage(storage);
  if (!target) return undefined;
  try {
    return normalizeLocale(target.getItem(LOCALE_STORAGE_KEY));
  } catch {
    return undefined;
  }
}

export function persistLocale(locale: Locale, storage?: LocaleStorage | null): boolean {
  const normalized = normalizeLocale(locale);
  const target = selectedStorage(storage);
  if (!normalized || !target) return false;
  try {
    target.setItem(LOCALE_STORAGE_KEY, normalized);
    return true;
  } catch {
    return false;
  }
}

export function detectLocale(options: LocaleDetectionOptions = {}): Locale {
  const persisted = readLocale(options.storage);
  if (persisted) return persisted;

  const documentLanguage = options.documentLanguage === undefined
    ? browserDocumentLanguage()
    : options.documentLanguage;
  const fromDocument = normalizeLocale(documentLanguage);
  if (fromDocument) return fromDocument;

  const navigatorLanguages = options.navigatorLanguages === undefined
    ? browserNavigatorLanguages()
    : options.navigatorLanguages ?? [];
  for (const language of navigatorLanguages) {
    const normalized = normalizeLocale(language);
    if (normalized) return normalized;
  }
  return normalizeLocale(options.fallback) ?? DEFAULT_LOCALE;
}

function interpolationIsSafe(value: unknown): value is TranslationValue {
  return typeof value === "string"
    || (typeof value === "number" && Number.isFinite(value));
}

function render(template: string, params: Record<string, unknown> | undefined): string {
  const parameters = Array.from(template.matchAll(PARAMETER_PATTERN), (match) => match[1]);
  if (parameters.length === 0) return template;
  if (!params || parameters.some((name) =>
    !Object.prototype.hasOwnProperty.call(params, name) || !interpolationIsSafe(params[name]))) {
    return TRANSLATION_FALLBACK;
  }
  return template.replace(PARAMETER_PATTERN, (_placeholder, name: string) =>
    String(params[name]));
}

export function translate<Key extends MessageKey>(
  locale: Locale,
  key: Key,
  ...args: TranslationArgs<Key>
): string {
  const selectedLocale = normalizeLocale(locale) ?? DEFAULT_LOCALE;
  const dictionary = MESSAGES[selectedLocale] as Record<string, string>;
  if (!Object.prototype.hasOwnProperty.call(dictionary, key)) {
    return TRANSLATION_FALLBACK;
  }
  return render(dictionary[key], args[0] as Record<string, unknown> | undefined);
}

export function createTranslator(locale: Locale): Translator {
  return ((key: MessageKey, params?: Record<string, unknown>) =>
    translate(locale, key, ...([params] as TranslationArgs<typeof key>))) as Translator;
}

function validDate(value: Date | string | number): Date | undefined {
  const date = value instanceof Date ? new Date(value.getTime()) : new Date(value);
  return Number.isFinite(date.getTime()) ? date : undefined;
}

export function formatDate(
  locale: Locale,
  value: Date | string | number,
  options: Intl.DateTimeFormatOptions = {},
): string {
  const date = validDate(value);
  if (!date) return TRANSLATION_FALLBACK;
  const selectedLocale = normalizeLocale(locale) ?? DEFAULT_LOCALE;
  try {
    return new Intl.DateTimeFormat(selectedLocale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      ...options,
    }).format(date);
  } catch {
    return TRANSLATION_FALLBACK;
  }
}

export function formatNumber(
  locale: Locale,
  value: number,
  options: Intl.NumberFormatOptions = {},
): string {
  if (!Number.isFinite(value)) return TRANSLATION_FALLBACK;
  const selectedLocale = normalizeLocale(locale) ?? DEFAULT_LOCALE;
  try {
    return new Intl.NumberFormat(selectedLocale, options).format(value);
  } catch {
    return TRANSLATION_FALLBACK;
  }
}

export function formatCount(locale: Locale, value: number): string {
  if (!Number.isFinite(value) || value < 0) return TRANSLATION_FALLBACK;
  return formatNumber(locale, Math.trunc(value), { maximumFractionDigits: 0 });
}

export function formatMessageCount(locale: Locale, value: number): string {
  const count = formatCount(locale, value);
  if (count === TRANSLATION_FALLBACK) return TRANSLATION_FALLBACK;
  const category = new Intl.PluralRules(normalizeLocale(locale) ?? DEFAULT_LOCALE)
    .select(Math.trunc(value));
  const key = category === "one" ? "inbox.messageCount.one" : "inbox.messageCount.other";
  return translate(locale, key, { count });
}

export function formatKilobytes(locale: Locale, bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return TRANSLATION_FALLBACK;
  const kilobytes = formatCount(locale, Math.ceil(bytes / 1024));
  return kilobytes === TRANSLATION_FALLBACK ? TRANSLATION_FALLBACK : `${kilobytes} KB`;
}
