import {
  defaultJournalCatalogView,
  journalCatalogCanFilterUnsubscribed,
  journalCatalogEmptyState,
  journalCatalogViewForSelection,
} from "./journalCatalog.ts";

const initial = defaultJournalCatalogView();
if (initial !== "subscribed") throw new Error("Journal entry must default to subscribed view");
if (journalCatalogViewForSelection(null) !== "subscribed") throw new Error("null collection selection must mean subscribed view");
const collection = journalCatalogViewForSelection("FT50");
if (JSON.stringify(collection) !== JSON.stringify({ kind: "collection", code: "FT50" })) throw new Error("collection selection should be explicit");
if (journalCatalogCanFilterUnsubscribed(initial)) throw new Error("unsubscribed filter must be disabled in subscribed view");
if (!journalCatalogCanFilterUnsubscribed(collection)) throw new Error("unsubscribed filter must work inside a collection");
if (!journalCatalogEmptyState(initial, 0).startsWith("尚未订阅期刊")) throw new Error("zero subscriptions need an actionable empty state");
if (journalCatalogEmptyState(initial, 3) !== "没有符合条件的期刊") throw new Error("subscribed view should use the normal filtered empty state");

console.log("journal catalog tests passed");
