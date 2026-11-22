import {
  useEffect,
  useMemo,
  useState,
  type ReactElement,
  type RefObject,
} from "react";
import { ChevronLeft } from "lucide-react";
import {
  ActionButton,
  Dialog,
  EmptyState,
  OwnedItemPicker,
  PanelTabs,
  QuantityControl,
  TextField,
} from "@spellwake/ui";
import {
  listingName,
  readWhole,
  termsProblems,
  type ListableAsset,
  type Operation,
} from "../../market/listing-sources";
import { ListingArt, COIN } from "./ListingDialogs";
import * as s from "./market.css";

type Tab = "cards" | "packs";
const detailOf = (asset: ListableAsset): { detail?: string } => {
  const detail = assetDetail(asset);
  return detail ? { detail } : {};
};
const assetDetail = (asset: ListableAsset): string | undefined =>
  asset.item.kind === "card"
    ? `${asset.item.setCode ?? asset.item.setName} · ${asset.item.finish}`
    : undefined;

/**
 * Create listing, as approved: first one owned card printing or pack product,
 * then its quantity and the total asking price. Back keeps the choice and the
 * unsent terms; X, Escape or a press outside discards the draft. Create
 * listing is the final step and holds the dialog open until the outcome is
 * known. Only what can be listed now is offered and counted.
 */
export function CreateListingDialog({
  open,
  assets,
  operation,
  canTrade,
  onCreate,
  onCheck,
  onDismissRejection,
  onClose,
  returnFocusRef,
}: {
  open: boolean;
  /** Null while what you own is being read. */
  assets: readonly ListableAsset[] | null;
  operation: Operation | null;
  canTrade: boolean;
  onCreate: (assetId: string, quantity: number, total: number) => void;
  onCheck: () => void;
  onDismissRejection: () => void;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
}): ReactElement {
  const [stage, setStage] = useState<"pick" | "terms">("pick");
  const [tab, setTab] = useState<Tab>("cards");
  const [queries, setQueries] = useState<Record<Tab, string>>({
    cards: "",
    packs: "",
  });
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [termsFor, setTermsFor] = useState<string | null>(null);
  const [quantity, setQuantity] = useState("1");
  const [total, setTotal] = useState("");
  const [isTotalTouched, setIsTotalTouched] = useState(false);

  // Each opening starts fresh at Cards with nothing chosen.
  useEffect(() => {
    if (!open) return;
    setStage("pick");
    setTab("cards");
    setQueries({ cards: "", packs: "" });
    setChosenId(null);
    setTermsFor(null);
    setQuantity("1");
    setTotal("");
    setIsTotalTouched(false);
  }, [open]);

  const held = operation?.kind === "create" ? operation : null;
  const stageKind = held?.stage.kind ?? null;
  const isFrozen = stageKind === "pending" || stageKind === "unconfirmed";
  const listable = useMemo(
    () => (assets ?? []).filter((asset) => asset.available > 0),
    [assets],
  );
  const chosen =
    held?.asset ?? listable.find((asset) => asset.id === chosenId) ?? null;
  const shownStage = held ? "terms" : stage;

  const next = () => {
    if (!chosen) return;
    // A different asset starts its terms afresh; the same one keeps them.
    if (termsFor !== chosen.id) {
      setTermsFor(chosen.id);
      setQuantity("1");
      setTotal("");
      setIsTotalTouched(false);
    }
    setStage("terms");
  };

  const available = chosen?.available ?? 0;
  const shownQuantity = held ? String(held.quantity) : quantity;
  const shownTotal = held ? String(held.total) : total;
  const problems = termsProblems(shownQuantity, shownTotal, available);
  const count = readWhole(shownQuantity);
  const step = typeof count === "number" ? count : 0;
  const isUnconfirmed = stageKind === "unconfirmed";

  const body =
    shownStage === "pick" ? (
      <PanelTabs
        label="Item type"
        treatment="pill"
        value={tab}
        onValueChange={(value) => setTab(value as Tab)}
        tabs={(["cards", "packs"] as const).map((kind) => {
          const plural = kind === "cards" ? "cards" : "packs";
          const needle = queries[kind].trim().toLowerCase();
          const ofKind = listable.filter((asset) =>
            kind === "cards"
              ? asset.item.kind === "card"
              : asset.item.kind === "pack",
          );
          const shown = ofKind.filter((asset) =>
            [
              listingName(asset.item),
              asset.item.setName,
              asset.item.setCode ?? "",
            ]
              .join(" ")
              .toLowerCase()
              .includes(needle),
          );
          return {
            value: kind,
            label: kind === "cards" ? "Cards" : "Packs",
            content: (
              <div className={s.pickerBody}>
                <TextField
                  type="search"
                  label={`Search ${plural}`}
                  placeholder={`Search ${plural}`}
                  hideLabel
                  treatment="compact-form"
                  value={queries[kind]}
                  onValueChange={(value) =>
                    setQueries((current) => ({ ...current, [kind]: value }))
                  }
                />
                <div className={s.pickerList}>
                  {assets === null ? (
                    <p className={s.pickerQuiet} role="status">
                      <span className={s.spinner} aria-hidden="true" />
                      Loading your {plural}…
                    </p>
                  ) : shown.length > 0 ? (
                    <OwnedItemPicker
                      mode="select"
                      label={`Your ${plural}`}
                      selectedId={chosenId}
                      onSelect={setChosenId}
                      items={shown.map((asset) => ({
                        id: asset.id,
                        name: listingName(asset.item),
                        kind: asset.item.kind,
                        ...detailOf(asset),
                        available: asset.available,
                        artwork: <ListingArt item={asset.item} />,
                      }))}
                    />
                  ) : (
                    <div className={s.pickerQuiet}>
                      <EmptyState
                        treatment="contained"
                        title={
                          needle && ofKind.length > 0
                            ? `No matching ${plural}`
                            : `No available ${plural}`
                        }
                      />
                    </div>
                  )}
                </div>
              </div>
            ),
          };
        })}
      />
    ) : chosen ? (
      <div className={s.terms}>
        <button
          type="button"
          className={s.backLink}
          disabled={isFrozen}
          onClick={() => setStage("pick")}
        >
          <ChevronLeft aria-hidden="true" />
          Back
        </button>
        <div className={s.termsItem}>
          <span className={s.termsArt} aria-hidden="true">
            <ListingArt item={chosen.item} />
          </span>
          <div className={s.termsCopy}>
            <h3 className={s.termsName}>{listingName(chosen.item)}</h3>
            {chosen.item.kind === "card" && <p>{assetDetail(chosen)}</p>}
            <p>{available} available</p>
          </div>
        </div>
        <div className={s.field}>
          <span className={s.fieldLabel} id="listing-quantity-label">
            Quantity
          </span>
          <div className={s.fieldControl}>
            <QuantityControl
              label="Quantity"
              value={step}
              treatment="joined"
              disabled={isFrozen}
              canDecrease={step > 1}
              canIncrease={step < available}
              onDecrease={() => setQuantity(String(Math.max(1, step - 1)))}
              onIncrease={() =>
                setQuantity(String(Math.min(available, step + 1)))
              }
              edit={{
                draft: shownQuantity,
                onChange: setQuantity,
                ...(problems.quantity ? { error: problems.quantity } : {}),
              }}
            />
            {problems.quantity && (
              <p className={s.fieldError} role="alert">
                {problems.quantity}
              </p>
            )}
          </div>
        </div>
        <div className={s.field}>
          <span className={s.fieldLabel}>Total price</span>
          <div className={s.fieldControl}>
            <span className={s.priceField}>
              <img className={s.priceCoin} src={COIN} alt="" />
              <TextField
                label="Total price"
                hideLabel
                inputMode="numeric"
                treatment="compact-form"
                value={shownTotal}
                disabled={isFrozen}
                onValueChange={setTotal}
                onBlur={() => setIsTotalTouched(true)}
                {...(isTotalTouched && problems.total
                  ? { error: problems.total }
                  : {})}
              />
            </span>
          </div>
        </div>
        {(stageKind === "unconfirmed" || stageKind === "rejected") && (
          <p
            className={s.stageLine}
            data-tone={isUnconfirmed ? "unknown" : "known"}
            role="status"
          >
            {held?.stage.kind === "rejected"
              ? held.stage.message
              : "Listing not confirmed."}
          </p>
        )}
      </div>
    ) : null;

  const create = () => {
    if (!chosen || problems.quantity || problems.total || !canTrade) {
      setIsTotalTouched(true);
      return;
    }
    onCreate(chosen.id, step, Number(shownTotal));
  };
  return (
    <Dialog
      open={open}
      title="Create listing"
      size="standard"
      className={s.createDialog}
      busy={isFrozen}
      returnFocusRef={returnFocusRef}
      onClose={() => {
        if (isFrozen) return;
        if (stageKind === "rejected") onDismissRejection();
        onClose();
      }}
      actions={
        shownStage === "pick" ? (
          <ActionButton
            treatment="compact-form"
            disabled={!chosen}
            onClick={next}
          >
            Next
          </ActionButton>
        ) : (
          <ActionButton
            treatment="compact-form"
            emphasis="primary"
            pending={
              stageKind === "pending" ||
              (held?.stage.kind === "unconfirmed" && held.stage.isChecking)
            }
            disabled={
              !isUnconfirmed &&
              (!canTrade ||
                !!problems.quantity ||
                (isTotalTouched && !!problems.total))
            }
            onClick={() => {
              if (isUnconfirmed) onCheck();
              else {
                if (stageKind === "rejected") onDismissRejection();
                create();
              }
            }}
          >
            {isUnconfirmed ? "Check status" : "Create listing"}
          </ActionButton>
        )
      }
    >
      {body}
    </Dialog>
  );
}
