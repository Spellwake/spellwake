import type { Purchase } from "@spellwake/application-contracts";
import type { PurchaseRow, PurchaseStatus } from "./account-sources";

const STATUS: Record<Purchase["state"], PurchaseStatus> = {
  creating: "Creating",
  pending: "Pending",
  paid: "Paid",
  canceled: "Canceled",
  refunded: "Refunded",
  reversed: "Reversed",
};

/**
 * A price as charged, such as "$2.99 USD". The amount arrives in the
 * currency's smallest unit, and how many of those make one whole unit depends
 * on the currency, so the divisor is read from the currency itself. Returns
 * null for a currency this device cannot format, which is never shown as a
 * guessed figure.
 */
export function purchaseAmount(price: Purchase["price"]): string | null {
  const code = price.currency.toUpperCase();
  try {
    const format = new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: code,
    });
    const digits = format.resolvedOptions().maximumFractionDigits ?? 2;
    return `${format.format(Number(price.minorUnits) / 10 ** digits)} ${code}`;
  } catch {
    return null;
  }
}

/**
 * Purchases as the history lists them, newest last as the server pages them.
 * An item whose name is not in the catalogue read is listed as "Cosmetic".
 */
export function purchaseRows(
  purchases: readonly Purchase[],
  names: ReadonlyMap<string, string>,
): PurchaseRow[] {
  return purchases.map((purchase) => ({
    id: purchase.purchaseId,
    item: names.get(purchase.cosmeticId) ?? "Cosmetic",
    date: purchase.createdAt,
    amount: purchaseAmount(purchase.price) ?? "—",
    status: STATUS[purchase.state],
  }));
}

/** A purchase date as the history shows it, such as "Nov 1, 2026". */
export function purchaseDate(instant: string): string {
  const time = Date.parse(instant);
  return Number.isFinite(time)
    ? new Intl.DateTimeFormat("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
        timeZone: "UTC",
      }).format(time)
    : "—";
}
