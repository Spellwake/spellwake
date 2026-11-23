import { useEffect, useMemo, useRef, useState, type ReactElement } from "react";
import { parseAsString, parseAsStringLiteral, useQueryStates } from "nuqs";
import {
  ActionButton,
  CoinAmount,
  EmptyState,
  FeedbackMessages,
  SelectMenu,
  SupplyIcon,
  TextField,
} from "@spellwake/ui";
import {
  AVAILABILITY_LABELS,
  boughtMessage,
  matchTiles,
  tileAvailability,
  type Availability,
  type SupplySource,
  type SupplyTile,
} from "../../market/supply-sources";
import { PackWrapper } from "../packs/PackWrapper";
import { COIN } from "./ListingDialogs";
import { BuyPacksDialog } from "./BuyPacksDialog";
import { VoteDialog } from "./VoteDialog";
import { useArrival } from "../../lib/arrival";
import * as s from "./supply.css";

const AVAILABILITIES = Object.keys(AVAILABILITY_LABELS) as Availability[];
const GAP_COPY = {
  "signed-out": {
    title: "Sign in to buy packs",
    description: "Packs you buy belong to your account.",
  },
  unregistered: {
    title: "Finish creating your account",
    description: "Choose a username to start collecting.",
  },
} as const;
const queryParsers = {
  q: parseAsString.withDefault(""),
  availability: parseAsStringLiteral(AVAILABILITIES).withDefault("all"),
};

function Tile({
  tile,
  onBuy,
}: {
  tile: SupplyTile;
  onBuy: (button: HTMLButtonElement) => void;
}): ReactElement {
  const availability = tileAvailability(tile);
  if (tile.kind === "set")
    return (
      <li className={s.tile} data-kind="set">
        <div className={s.bare}>
          <span className={s.bareCode} aria-hidden="true">
            {tile.setCode ?? tile.setName}
          </span>
          <h3 className={s.bareName}>{tile.setName}</h3>
          <ActionButton className={s.tileAction} disabled>
            Unavailable
          </ActionButton>
        </div>
      </li>
    );
  return (
    <li className={s.tile} data-kind="product">
      <span className={s.art} aria-hidden="true">
        <PackWrapper setName={tile.setName} product={tile.fullName} />
      </span>
      <div className={s.copy}>
        <h3 className={s.name}>
          {tile.setName}
          <span className={s.product}>{tile.product}</span>
        </h3>
        {availability === "checking" ? (
          <p className={s.checking}>
            {tile.quoteState === "failed"
              ? "Price unavailable"
              : "Checking availability…"}
          </p>
        ) : availability === "available" && tile.quote ? (
          <>
            <p className={s.remaining}>{tile.quote.remaining} remaining</p>
            <p className={s.price}>
              <CoinAmount amount={tile.quote.total} iconSrc={COIN} />
            </p>
          </>
        ) : null}
        <ActionButton
          className={s.tileAction}
          disabled={availability !== "available"}
          aria-haspopup="dialog"
          aria-label={
            availability === "available" ? `Buy ${tile.fullName}` : undefined
          }
          onClick={(event) => onBuy(event.currentTarget)}
        >
          {availability === "sold-out" ? "Sold out" : "Buy"}
        </ActionButton>
      </div>
    </li>
  );
}

/**
 * Market Supply, as approved (market-supply-approval-v1): the vote banner,
 * search and Availability, then every set as pack-product tiles; a set with
 * no qualified product stays a quiet name with Unavailable. Buy packs asks
 * for a quantity against a fresh quote; the vote chooses up to ten sets.
 * Both hold open until their outcome is known.
 */
export function SupplyPage({ source }: { source: SupplySource }): ReactElement {
  const [params, setParams] = useQueryStates(queryParsers);
  const [buyingId, setBuyingId] = useState<string | null>(null);
  const [isVoting, setIsVoting] = useState(false);
  const buyTrigger = useRef<HTMLElement | null>(null);
  const voteButton = useRef<HTMLButtonElement>(null);
  const ready = source.status === "ready" ? source : null;
  const shown = useMemo(
    () => (ready ? matchTiles(ready.tiles, params.q, params.availability) : []),
    [ready, params.q, params.availability],
  );

  const arrival = useArrival(shown.length > 0);

  // A purchase in flight or unconfirmed keeps its dialog, even after a reload.
  const purchaseTile = ready?.purchase?.tileId ?? null;
  const openTileId = purchaseTile ?? buyingId;
  const openTile =
    ready?.tiles.find(
      (tile) => tile.id === openTileId && tile.kind === "product",
    ) ?? null;
  const completed = ready?.completed ?? null;
  useEffect(() => {
    if (completed?.kind === "bought") setBuyingId(null);
    if (completed?.kind === "voted") setIsVoting(false);
  }, [completed]);
  const ballot = ready?.ballot ?? null;
  const isBallotHeld =
    ballot?.status === "ready" &&
    ballot.save !== null &&
    ballot.save.kind !== "rejected";

  const messages = [
    ...(ready?.failure
      ? [{ id: "read", text: ready.failure, onDismiss: ready.dismissFailure }]
      : []),
    ...(completed && ready
      ? [
          {
            id: "done",
            tone: "success" as const,
            text:
              completed.kind === "bought"
                ? boughtMessage(completed.setName, completed.quantity)
                : "Vote saved.",
            onDismiss: ready.dismissCompleted,
          },
        ]
      : []),
  ];
  const hasVote =
    ballot?.status === "ready" && ballot.saved.length > 0 && ballot.isOpen;

  return (
    <section className={s.page} aria-label="Supply" role="region">
      {ballot && ballot.status !== "unavailable" && (
        <div className={s.banner}>
          <SupplyIcon className={s.bannerIcon} aria-hidden="true" />
          <div className={s.bannerCopy}>
            <h2 className={s.bannerTitle}>Choose the next supply drop</h2>
            {ballot.status === "ready" && (
              <p className={s.bannerDetail}>
                {ballot.isOpen
                  ? `Voting closes ${ballot.closes}`
                  : ballot.results
                    ? "Results published"
                    : "Voting closed"}
                {"  ·  "}Next drop {ballot.nextDrop}
              </p>
            )}
          </div>
          <ActionButton
            ref={voteButton}
            className={s.bannerAction}
            aria-haspopup="dialog"
            disabled={ballot.status === "loading"}
            onClick={() => setIsVoting(true)}
          >
            {ballot.status === "ready" && !ballot.isOpen
              ? ballot.results
                ? "See results"
                : "See your vote"
              : hasVote
                ? "Edit vote"
                : "Vote for next drop"}
          </ActionButton>
        </div>
      )}
      <div className={s.toolbar}>
        <div className={s.search}>
          <TextField
            type="search"
            label="Search all sets"
            placeholder="Search all sets…"
            value={params.q}
            onValueChange={(value) => void setParams({ q: value || null })}
            hideLabel
            treatment="compact-form"
          />
        </div>
        <SelectMenu
          label="Availability"
          valuePrefix="Availability: "
          value={params.availability}
          options={AVAILABILITIES.map((value) => ({
            value,
            label: AVAILABILITY_LABELS[value],
          }))}
          onValueChange={(value) =>
            void setParams({
              availability: value === "all" ? null : (value as Availability),
            })
          }
          disabled={!ready}
        />
      </div>

      {shown.length > 0 ? (
        <>
          <ul className={s.gallery} aria-label="Sets" {...arrival.group}>
            {shown.map((tile) => (
              <Tile
                key={tile.id}
                tile={tile}
                onBuy={(button) => {
                  buyTrigger.current = button;
                  setBuyingId(tile.id);
                }}
              />
            ))}
          </ul>
          {ready?.isAppending && (
            <p className={s.appending} role="status">
              <span className={s.spinner} aria-hidden="true" />
              Loading more sets…
            </p>
          )}
        </>
      ) : (
        <div className={s.quiet}>
          {source.status === "loading" ||
          (ready?.isAppending && ready.tiles.length === 0) ? (
            <p className={s.loading} role="status">
              <span className={s.spinner} aria-hidden="true" />
              Loading sets…
            </p>
          ) : source.status === "signed-out" ||
            source.status === "unregistered" ? (
            <EmptyState {...GAP_COPY[source.status]} />
          ) : source.status === "unavailable" ||
            ready?.isUnread ? null : ready && ready.tiles.length === 0 ? (
            <EmptyState title="No sets available" />
          ) : (
            <EmptyState
              title="No matching sets"
              action={
                <ActionButton
                  treatment="compact-form"
                  onClick={() =>
                    void setParams({ q: null, availability: null })
                  }
                >
                  Clear filters
                </ActionButton>
              }
            />
          )}
        </div>
      )}

      {ready && (
        <>
          <BuyPacksDialog
            open={openTile !== null}
            tile={openTile}
            source={ready}
            returnFocusRef={buyTrigger}
            onClose={() => {
              ready.dismissPurchase();
              setBuyingId(null);
            }}
          />
          {ready.ballot && (
            <VoteDialog
              open={isVoting || isBallotHeld}
              ballot={ready.ballot}
              onSave={ready.saveBallot}
              onCheck={ready.checkBallot}
              onClose={() => {
                ready.dismissBallot();
                setIsVoting(false);
              }}
              returnFocusRef={voteButton}
            />
          )}
        </>
      )}
      <FeedbackMessages
        label="Supply messages"
        presentation="toast"
        items={messages}
      />
    </section>
  );
}
