import {
  closedScenes,
  isClosedView,
  TradeClosedSpecimen,
  tradeClosedUses,
} from "./TradeClosedSpecimen";
import type { TradeClosure } from "@spellwake/ui";
import {
  TradeReceiptSpecimen,
  receiptFromHistory,
  receiptUses,
  type ReceiptExample,
} from "./TradeReceiptSpecimen";
import {
  actionScenes,
  isTradeActionView,
  tradeActionUses,
} from "./TradeActionPanel";
import {
  lazy,
  Suspense,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { ArrowUpRight, Layers2 } from "lucide-react";
import { DeckControls, ImportControls, PredictionControls } from "./specimens";
import {
  locationView,
  originalUrl,
  references,
  surfaces,
  views,
  type View,
} from "./references";
import {
  TablePreferences,
  PictureZoom,
  BadgeChoice,
  preferenceUses,
} from "./PreferenceSpecimens";
import { FilterSpecimen, filterUses } from "./FilterSpecimens";
import { AmountSpecimen, amountUses } from "./AmountSpecimens";
import {
  FeedbackSpecimen,
  emptyStateUse,
  feedbackUses,
} from "./FeedbackSpecimens";
import { NavigationSpecimen, navigationUses } from "./NavigationSpecimens";
import { popupUses } from "./popupExamples";
import { IdentitySpecimen, identityUses } from "./IdentitySpecimens";
import { RankSpecimen, rankUses } from "./RankSpecimens";
import { UsageMap, type ControlUse } from "./UsageMap";
import { MatchHudSpecimen, hudUses } from "./MatchHudSpecimens";
import { AllocationSpecimen, allocationUses } from "./AllocationSpecimen";
import {
  ConversationEventSpecimen,
  eventUses,
} from "./ConversationEventSpecimen";
import {
  ConfirmationSpecimen,
  ConfirmationExample,
  confirmationUses,
  isConfirmationView,
} from "./ConfirmationSpecimen";
import {
  OwnedPickerSpecimen,
  isOwnedPickerView,
  pickerUses,
} from "./OwnedPickerSpecimen";
import {
  TradeReadinessSpecimen,
  isReadinessView,
  readinessUses,
} from "./TradeReadinessSpecimen";
import {
  TradeItemSpecimen,
  isTradeItemView,
  tradeItemUses,
} from "./TradeItemSpecimen";
import {
  TradeTraySpecimen,
  isTradeTrayView,
  tradeTrayUses,
} from "./TradeTraySpecimen";
import {
  EditableTraySpecimen,
  isEditableTrayView,
  editableTrayUses,
} from "./EditableTraySpecimen";
import {
  PickerDialogSpecimen,
  PickerDialogExample,
  isPickerDialogView,
  pickerDialogUses,
} from "./PickerDialogSpecimen";
import { ListingSpecimen, isListingView, listingUses } from "./ListingSpecimen";
import { PeopleRailSpecimen, isRailView, railUses } from "./PeopleRailSpecimen";
import { ConversationSpecimen, conversationUses } from "./ConversationSpecimen";
import { AccountEntrySpecimen, accountEntryUses } from "./AccountEntrySpecimen";
import { CastingCostSpecimen, castingCostUses } from "./CastingCostSpecimen";
import { TargetingSpecimen, targetingUses } from "./TargetingSpecimen";
import "./gallery.css";
import "./combat-preview.css";

const PopupSpecimen = lazy(() => import("./PopupSpecimens"));

const ModeSpecimen = lazy(() => import("./ModeSpecimens"));
const StackSpecimen = lazy(() => import("./StackSpecimens"));
const stackUses: ControlUse[] = [
  {
    id: "spell-stack",
    name: "Compact spell stack",
    approvedCount: 25,
    description:
      "Overlapping spells and abilities, older-item inspection, quiet waiting and an empty or remaining pile.",
    screens: [
      "AS01:stack",
      "AS03:stack",
      "AS05:stack",
      "AS06:stack",
      "AS07:stack",
      "AS07:long-trigger",
      "AS08:stack",
    ],
  },
  {
    id: "target-connection",
    name: "Target connection",
    approvedCount: 26,
    description:
      "A selected spell points to its target in the pile or to a player portrait on the board.",
    screens: ["AS02:stack", "AS04:stack"],
  },
];
const modeUses: ControlUse[] = [
  {
    id: "card-face",
    name: "Original card face",
    description:
      "Full original image for choosing modes and showing the spell during targeting.",
    approvedCount: 62,
    screens: ["MC01:modes", "MC04:face", "MC05:face", "MM06:face"],
  },
  {
    id: "printed-mode-picker",
    name: "Printed mode picker",
    description:
      "One original face per option. Only the outlined printed paragraph accepts a choice.",
    approvedCount: 10,
    screens: [
      "MC01:modes",
      "MC02:modes",
      "MM01:modes",
      "MM02:modes",
      "MM03:modes",
      "MM05:modes",
    ],
  },
];
function App() {
  const [view, setView] = useState(locationView);
  const focusAfterNavigation = useRef<string | undefined>(undefined);
  useEffect(() => {
    const target = focusAfterNavigation.current;
    if (target) document.getElementById(target)?.focus();
    focusAfterNavigation.current = undefined;
  }, [view]);
  useEffect(() => {
    const onPop = () => setView(locationView());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  function navigate(
    next: View,
    focusId?: string,
    example?: {
      receipt?: ReceiptExample;
      completedTrade?: boolean;
      closure?: TradeClosure;
      replace?: boolean;
    },
  ) {
    if (next === view) return;
    focusAfterNavigation.current = focusId;
    const url = new URL(window.location.href);
    url.searchParams.set("view", next);
    const { replace, ...entry } = example ?? {};
    if (replace) window.history.replaceState({ from: view, ...entry }, "", url);
    else window.history.pushState({ from: view, ...entry }, "", url);
    setView(next);
  }
  function openReceipt(
    source: ReceiptExample["source"],
    quantities: ReceiptExample["quantities"],
  ) {
    navigate("TR08:receipt", "receipt-heading", {
      receipt: { source, quantities },
    });
  }
  function showCanceled() {
    navigate("TR10:canceled", "closed-trade-heading", { replace: true });
  }
  const closed = isClosedView(view);
  const receipt = view === "TR08:receipt";
  const ref = references[view];
  const preferences = [
    "Table settings",
    "Picture zoom",
    "Badge choice",
  ].includes(ref.surface);
  const filters = [
    "Collection filters",
    "Listing filters",
    "Supply vote",
  ].includes(ref.surface);
  const amounts = ref.surface === "Amounts";
  const feedback = ref.surface === "Messages";
  const navigation = ref.surface === "Navigation";
  const popups = ref.surface === "Menus and tooltips";
  const identities = ref.surface === "Player identity";
  const ranks = ref.surface === "Ranks and loading";
  const printedModes = ref.surface === "Printed card modes";
  const stack = ref.surface === "Spell stack";
  const hud = ref.surface === "Match turn controls";
  const allocation = ref.surface === "Damage allocation";
  const targeting = ref.surface === "Spell targeting";
  const castingCost = ref.surface === "Casting costs";
  const accountEntry = ref.surface === "Account entry";
  const pickerDialog = isPickerDialogView(view);
  const availability = view === "TR09:availability";
  const originalTradeReference =
    availability || view === "TR09:empty-tray" || pickerDialog;
  const tradeActions = isTradeActionView(view);
  const editingTray = isEditableTrayView(view);
  const tradeTrays = isTradeTrayView(view);
  const tradeItems = isTradeItemView(view);
  const readiness = isReadinessView(view);
  const ownedPicker = isOwnedPickerView(view);
  const listings = isListingView(view);
  const confirmations = isConfirmationView(view);
  const people = isRailView(view);
  const events = ref.surface === "Conversation events";
  const conversation = ref.surface === "Conversation controls";
  const wideComparison =
    closed ||
    receipt ||
    pickerDialog ||
    tradeActions ||
    editingTray ||
    tradeTrays ||
    tradeItems ||
    readiness ||
    ownedPicker ||
    listings ||
    confirmations ||
    people ||
    events ||
    conversation ||
    accountEntry ||
    castingCost ||
    targeting ||
    printedModes ||
    stack ||
    hud ||
    allocation;
  const nativeScale = popups || identities || ranks || wideComparison;
  const uses = closed
    ? tradeClosedUses
    : receipt
      ? receiptUses
      : tradeActions
        ? tradeActionUses
        : pickerDialog
          ? pickerDialogUses
          : editingTray
            ? editableTrayUses
            : tradeTrays
              ? tradeTrayUses
              : tradeItems
                ? tradeItemUses
                : readiness
                  ? readinessUses
                  : ownedPicker
                    ? pickerUses
                    : listings
                      ? listingUses
                      : confirmations
                        ? confirmationUses
                        : people
                          ? railUses
                          : events
                            ? eventUses
                            : conversation
                              ? conversationUses
                              : accountEntry
                                ? accountEntryUses
                                : castingCost
                                  ? castingCostUses
                                  : targeting
                                    ? targetingUses
                                    : allocation
                                      ? allocationUses
                                      : hud
                                        ? hudUses
                                        : stack
                                          ? stackUses
                                          : printedModes
                                            ? modeUses
                                            : ranks
                                              ? rankUses
                                              : identities
                                                ? identityUses
                                                : popups
                                                  ? popupUses
                                                  : navigation
                                                    ? navigationUses
                                                    : feedback
                                                      ? feedbackUses
                                                      : amounts
                                                        ? amountUses
                                                        : filters
                                                          ? filterUses
                                                          : preferences
                                                            ? preferenceUses
                                                            : undefined;
  const fullStrip =
    navigation && view.startsWith("ST-") && view.endsWith(":tabs");
  const original = originalUrl(view);
  const [width, height] = ref.size;
  return (
    <>
      <header className="top">
        <div className="shell mast">
          <a href="../index.html" className="brand">
            Spellwake
          </a>
          <span className="rule" />
          <span>Design system</span>
          <a className="map" href="?review=DL-01">
            Full screens
          </a>
          <a href="../breakdown.html#shared">
            Component map <ArrowUpRight size={15} />
          </a>
        </div>
      </header>
      <main className="shell">
        <div className="intro">
          <h1>Shared controls</h1>
          <p>
            {closed
              ? view === "TR10:unavailable"
                ? "The reference shows the unavailable title. The live notice includes the shared Back action."
                : "Confirm Cancel trade in an open room to reach this notice. Back returns to the conversation."
              : receipt
                ? "Review what each player gave. Done returns to the conversation in this example."
                : tradeActions
                  ? "Try the trade actions, then simulate the other player’s response."
                  : view.endsWith(":locked-tray")
                    ? "Choose Edit items to reopen the offer or keep it ready."
                    : availability
                      ? "Try entering more than 2 copies or 1000 coins. The fields stop at the amount you own."
                      : view === "TR09:empty-tray"
                        ? "Open Add items to add cards, packs or coins to the empty offer."
                        : view === "TR09:no-matches"
                          ? "Clear the search to see your cards, or switch to Packs."
                          : view === "TR09:empty-category"
                            ? "This collection has no cards. Switch to Packs to see what you can add."
                            : pickerDialog
                              ? "Choose Cards, Packs or Coins and use the plus to add one. Close the picker to keep your selections."
                              : editingTray
                                ? "Change quantities, remove a row, or open Add items. Changes apply to this example."
                                : tradeTrays
                                  ? "Compare the partner’s offer with the two sides of the completed trade."
                                  : tradeItems
                                    ? "Edit the copy count in your offer. Compare it with the locked offer and receipt."
                                    : readiness
                                      ? "Change the state to compare each player’s badge and the shared status beneath the trays."
                                      : ownedPicker
                                        ? "Choose a holding for a listing, or add a copy to the example trade."
                                        : listings
                                          ? "Select a listing to inspect its lot. Compare the same item in the buy and cancel summaries."
                                          : confirmations
                                            ? "Try either action inside the dialog. Each example runs locally and can be reopened."
                                            : people
                                              ? "Select a friend, Requests or Find People. The approved rail stays alongside for comparison."
                                              : events
                                                ? "Try a decision. Trade and Join open the approved destination screen in another tab."
                                                : conversation
                                                  ? "Send a local example with Enter, or use Shift+Enter for a new line."
                                                  : accountEntry
                                                    ? "Try a provider or enter the six-digit example code. Each comparison uses the approved screen’s pixel dimensions."
                                                    : castingCost
                                                      ? "Compare the payment controls and change Blaze’s X to see its total cost."
                                                      : targeting
                                                        ? "Choose a highlighted creature to try the target offer."
                                                        : allocation
                                                          ? "Open the shared combat assignment or adjust Arc Lightning’s split."
                                                          : hud
                                                            ? "Hover or focus a phase to see its name. Try the offered action and compare each state at game size."
                                                            : stack
                                                              ? "Click a card to inspect it. Use the arrow keys or scroll over the pile to read older items. Escape closes inspection."
                                                              : printedModes
                                                                ? "Choose an outlined paragraph. The live example and approved crop scroll together at native size."
                                                                : ranks
                                                                  ? "Compare season ranks, compact rank emblems and loading placeholders at native size."
                                                                  : identities
                                                                    ? "Choose a request, open EmberFox’s profile, or switch between match seats."
                                                                    : popups
                                                                      ? "Open a menu or focus a hint to compare it with the approved screen at native size."
                                                                      : navigation
                                                                        ? "Section links and peer tabs in the approved Market and League screens."
                                                                        : feedback
                                                                          ? "Empty states and feedback messages across the approved screens."
                                                                          : amounts
                                                                            ? "Quantity controls and coin amounts across the approved screens."
                                                                            : filters
                                                                              ? "Checkboxes and removable filter chips across Collection, Listings, and Supply."
                                                                              : preferences
                                                                                ? "Range slider, setting switch, and radio group in approved desktop settings."
                                                                                : "Action button, text field, and select menu in approved desktop screens."}
          </p>
        </div>
        <nav aria-label="Preview screen" className="tabs">
          {surfaces.map((surface) => (
            <button
              type="button"
              key={surface}
              aria-current={ref.surface === surface ? "page" : undefined}
              onClick={() => {
                const next = views.find(
                  (id) => references[id].surface === surface,
                );
                if (next) navigate(next);
              }}
            >
              {surface}
            </button>
          ))}
        </nav>
        <div
          onScrollCapture={
            wideComparison
              ? (event) => {
                  const source = event.target;
                  if (!(source instanceof HTMLElement)) return;
                  const peer = source.matches(".popup-stage")
                    ? ".reference-inner"
                    : source.matches(".reference-inner")
                      ? ".popup-stage"
                      : null;
                  const pane = peer
                    ? event.currentTarget.querySelector<HTMLElement>(peer)
                    : null;
                  if (pane && Math.abs(pane.scrollLeft - source.scrollLeft) > 1)
                    pane.scrollLeft = source.scrollLeft;
                }
              : undefined
          }
          style={
            nativeScale
              ? ({ "--sample-height": ref.crop[3] + "px" } as CSSProperties)
              : undefined
          }
          className={
            nativeScale
              ? "preference-comparison popup-comparison" +
                (wideComparison ? " mode-comparison" : "") +
                (stack ? " stack-comparison" : "")
              : uses
                ? fullStrip
                  ? "preference-comparison navigation-wide"
                  : "preference-comparison"
                : "original-comparison"
          }
        >
          <section className="live" aria-labelledby="live-heading">
            <div className="section-head">
              <h2 id="live-heading">
                <span className="live-dot" />
                Live controls
              </h2>
              <label className="preset">
                <span id="preview-state-label">State</span>
                <select
                  aria-labelledby="preview-state-label"
                  value={view}
                  onChange={(event) =>
                    navigate(event.currentTarget.value as View)
                  }
                >
                  {views
                    .filter((id) => references[id].surface === ref.surface)
                    .map((id) => (
                      <option key={id} value={id}>
                        {references[id].state}
                      </option>
                    ))}
                </select>
              </label>
            </div>
            {closed ? (
              <TradeClosedSpecimen
                key={view}
                view={view}
                onBack={() =>
                  navigate("MSG01:trade", "conversation-event", {
                    closure: closedScenes[view],
                  })
                }
              />
            ) : receipt ? (
              <TradeReceiptSpecimen
                example={receiptFromHistory(window.history.state?.receipt)}
                onDone={() =>
                  navigate("MSG01:trade", "conversation-event", {
                    completedTrade: true,
                  })
                }
              />
            ) : tradeActions ? (
              <EditableTraySpecimen
                onCanceled={showCanceled}
                onComplete={openReceipt}
                key={view}
                view={actionScenes[view]}
                actionsView={view}
              />
            ) : pickerDialog ? (
              <PickerDialogSpecimen view={view} />
            ) : editingTray ? (
              <EditableTraySpecimen
                onCanceled={showCanceled}
                key={view}
                view={view}
                onComplete={openReceipt}
              />
            ) : tradeTrays ? (
              <TradeTraySpecimen view={view} />
            ) : tradeItems ? (
              <TradeItemSpecimen view={view} key={view} />
            ) : readiness ? (
              <TradeReadinessSpecimen view={view} />
            ) : ownedPicker ? (
              <OwnedPickerSpecimen view={view} key={view} />
            ) : listings ? (
              <ListingSpecimen key={view} view={view} />
            ) : confirmations ? (
              <ConfirmationSpecimen view={view} />
            ) : people ? (
              <PeopleRailSpecimen key={view} view={view} />
            ) : events ? (
              <ConversationEventSpecimen
                key={view}
                view={view}
                closure={window.history.state?.closure}
                completedTrade={window.history.state?.completedTrade === true}
                openTrade={() => navigate("TR01:own-tray", "trade-offer")}
              />
            ) : conversation ? (
              <ConversationSpecimen key={view} view={view} />
            ) : accountEntry ? (
              <AccountEntrySpecimen key={view} view={view} />
            ) : castingCost ? (
              <CastingCostSpecimen key={view} view={view} />
            ) : targeting ? (
              <TargetingSpecimen key={view} view={view} />
            ) : allocation ? (
              <AllocationSpecimen key={view} view={view} />
            ) : hud ? (
              <MatchHudSpecimen view={view} navigate={navigate} />
            ) : stack ? (
              <Suspense
                fallback={<div className="popup-stage" aria-busy="true" />}
              >
                <StackSpecimen key={view} view={view} />
              </Suspense>
            ) : printedModes ? (
              <Suspense
                fallback={<div className="popup-stage" aria-busy="true" />}
              >
                <ModeSpecimen key={view} view={view} navigate={navigate} />
              </Suspense>
            ) : ranks ? (
              <RankSpecimen view={view} />
            ) : identities ? (
              <IdentitySpecimen
                view={view}
                navigate={(next) =>
                  navigate(
                    next,
                    next.startsWith("NP-") ? undefined : "identity-preview",
                  )
                }
              />
            ) : popups ? (
              <Suspense
                fallback={<div className="popup-stage" aria-busy="true" />}
              >
                <PopupSpecimen key={view} view={view} />
              </Suspense>
            ) : navigation ? (
              <NavigationSpecimen view={view} navigate={navigate} />
            ) : feedback ? (
              <FeedbackSpecimen
                key={view}
                view={view}
                clearFilters={() => navigate("DL-01", "deck-search")}
              />
            ) : amounts ? (
              <AmountSpecimen key={view} view={view} />
            ) : filters ? (
              <FilterSpecimen key={view} view={view} />
            ) : ref.surface === "Deck library" ? (
              <DeckControls
                key={view}
                view={view}
                navigate={(next) => navigate(next, "deck-link")}
              />
            ) : ref.surface === "Predictions" ? (
              <PredictionControls />
            ) : ref.surface === "Table settings" ? (
              <TablePreferences key={view} view={view} />
            ) : ref.surface === "Picture zoom" ? (
              <PictureZoom />
            ) : ref.surface === "Badge choice" ? (
              <BadgeChoice />
            ) : (
              <ImportControls
                key={view}
                view={view}
                cancel={() => navigate("DL-01", "open-deck-import")}
              />
            )}
          </section>
          <section className="reference" aria-labelledby="ref-heading">
            <div className="section-head">
              <h2 id="ref-heading">
                <Layers2 size={17} />
                {originalTradeReference
                  ? "Original trade reference"
                  : view === "AS07:long-trigger"
                    ? "Stack layout reference"
                    : view === "X:cost"
                      ? "Approved X-value concept"
                      : "Approved reference"}{" "}
                <small>{view.split(":")[0]}</small>
              </h2>
              <a href={original} target="_blank" rel="noreferrer">
                Full screen <ArrowUpRight size={15} />
              </a>
            </div>
            <div className="reference-inner">
              <svg
                role="img"
                aria-label={
                  originalTradeReference
                    ? "Trade layout before the coin and quantity revisions"
                    : view === "AS07:long-trigger"
                      ? "Approved stack layout with Soul Warden"
                      : ref.surface + ", " + ref.state + ", approved controls"
                }
                viewBox={ref.crop.join(" ")}
                style={
                  nativeScale
                    ? { width: ref.crop[2], height: ref.crop[3], flexShrink: 0 }
                    : { maxWidth: ref.crop[2] }
                }
              >
                <image href={original} width={width} height={height} />
              </svg>
            </div>
            <details key={view}>
              <summary>Show full composition</summary>
              <img
                loading="lazy"
                src={original}
                alt={ref.surface + " approved desktop composition"}
              />
            </details>
          </section>
        </div>
        {uses && (
          <UsageMap
            uses={
              view.startsWith("TR09:") && !availability
                ? [...uses, emptyStateUse]
                : uses
            }
          />
        )}
      </main>
    </>
  );
}
export default function GalleryApp() {
  const query = new URLSearchParams(location.search);
  const pickerPreview = query.get("picker");
  const confirmation = query.get("confirmation");
  if (isPickerDialogView(pickerPreview)) {
    return <PickerDialogExample view={pickerPreview} />;
  }
  if (isConfirmationView(confirmation)) {
    return <ConfirmationExample view={confirmation} />;
  }
  return <App />;
}
