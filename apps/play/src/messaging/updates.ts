import { notificationSchema } from "@spellwake/application-contracts";

/** What an update is about, where the app knows; anything else is shown as sent. */
export type UpdateKind = "trade-offer" | "trade-accepted" | "sold" | "other";

/** One update as the Updates thread shows it. */
export type Update = {
  id: string;
  kind: UpdateKind;
  /** The server's own short line: "Trade offer received". */
  text: string;
  /** What the update is about: a trade or a listing. */
  sourceRef: string;
  /** Unread when it was read from the server; stays so for the visit that marks it. */
  isNew: boolean;
};

const KIND: Record<string, UpdateKind> = {
  trade_offer: "trade-offer",
  trade_accepted: "trade-accepted",
  market_sold: "sold",
};

/**
 * The updates that read as the contract says, unread first. The server keeps
 * no time for an update, so within each group they keep the order it sent.
 */
export function updates(items: readonly unknown[]): Update[] {
  const read = items.flatMap((item) => {
    const parsed = notificationSchema.safeParse(item);
    if (!parsed.success) return [];
    const { id, sourceKind, sourceRef, body, readAt } = parsed.data;
    return [
      {
        id,
        kind: KIND[sourceKind] ?? "other",
        text: body,
        sourceRef,
        isNew: readAt === null,
      },
    ];
  });
  return [
    ...read.filter((update) => update.isNew),
    ...read.filter((update) => !update.isNew),
  ];
}

/** The unread updates to mark read, a hundred at most, as the server takes them. */
export function toMarkRead(list: readonly Update[]): string[] {
  return list
    .filter((update) => update.isNew)
    .slice(0, 100)
    .map((update) => update.id);
}

/** Where an update leads: a trade's room, your listings, or nowhere. */
export type UpdateTarget =
  { to: "trade"; tradeId: string } | { to: "listings" } | null;
export function targetOf(update: Update): UpdateTarget {
  if (update.kind === "trade-offer" || update.kind === "trade-accepted")
    return { to: "trade", tradeId: update.sourceRef };
  if (update.kind === "sold") return { to: "listings" };
  return null;
}

/** The other person in a trade, from the side the viewer is on. */
export function otherParty(
  trade: { sender: { id: string }; recipient: { id: string } },
  accountId: string,
): string {
  return trade.sender.id === accountId ? trade.recipient.id : trade.sender.id;
}
