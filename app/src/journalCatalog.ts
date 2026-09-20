export type JournalCatalogView = "subscribed" | { kind: "collection"; code: string };

/** A journal page entry always starts from the user's current subscriptions. */
export function defaultJournalCatalogView(): JournalCatalogView {
  return "subscribed";
}

export function journalCatalogViewForSelection(code: string | null): JournalCatalogView {
  return code == null ? defaultJournalCatalogView() : { kind: "collection", code };
}

export function journalCatalogCanFilterUnsubscribed(view: JournalCatalogView): boolean {
  return view !== "subscribed";
}

export function journalCatalogEmptyState(view: JournalCatalogView, subscribedCount: number): string {
  if (view === "subscribed" && subscribedCount === 0) return "尚未订阅期刊，可从常用期刊集合或手动添加。";
  return "没有符合条件的期刊";
}
