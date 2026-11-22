import {
  useEffect,
  useId,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
  type RefObject,
} from "react";
import { AnimatePresence, motion } from "motion/react";
import { Check } from "lucide-react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  LoadingShape,
  LoadingState,
} from "@spellwake/ui";
import type {
  AccountSettingsSource,
  PurchaseHistory,
} from "../../account/account-sources";
import { Portrait } from "../../account/Portrait";
import { purchaseDate } from "../../account/purchase-rows";
import { motionLanguage as m } from "../../lib/motion";
import { BadgeEditor } from "./BadgeEditor";
import { useCommand, useNotice } from "./operation";
import { PictureEditor } from "./PictureEditor";
import { SectionTabs } from "./SectionTabs";
import * as s from "./account.css";

export const ACCOUNT_SECTIONS = [
  "profile",
  "patreon",
  "billing",
  "account",
] as const;
export type AccountSection = (typeof ACCOUNT_SECTIONS)[number];
const SECTION_LABEL: Record<AccountSection, string> = {
  profile: "Profile",
  patreon: "Patreon",
  billing: "Billing & purchases",
  account: "Account",
};
type Ready = Extract<AccountSettingsSource, { status: "ready" }>;

/** The mark Patreon supplies, drawn in the page's ink from the supplied file. */
function PatreonMark(): ReactElement {
  const url = `url("${import.meta.env.BASE_URL}account/patreon.svg")`;
  return (
    <span
      aria-hidden="true"
      className={s.providerMark}
      style={{
        background: "currentColor",
        maskImage: url,
        WebkitMaskImage: url,
        maskSize: "contain",
        WebkitMaskSize: "contain",
        maskRepeat: "no-repeat",
        WebkitMaskRepeat: "no-repeat",
      }}
    />
  );
}

function ProfileSection({
  source,
  onChangePicture,
  onChangeBadge,
}: {
  source: Ready;
  onChangePicture: () => void;
  onChangeBadge: () => void;
}): ReactElement {
  return (
    <div className={s.person}>
      <Portrait
        portrait={source.portrait}
        badge={source.badge}
        diameter={196}
        label={
          source.badge === "supporter"
            ? "Your picture, with the Supporter badge"
            : "Your picture"
        }
      />
      <div className={s.personCopy}>
        <h3 className={s.username}>{source.username}</h3>
        <div className={s.personActions}>
          <ActionButton
            className={s.action}
            disabled={source.picture === null}
            onClick={onChangePicture}
          >
            Change picture
          </ActionButton>
          <ActionButton
            className={s.action}
            disabled={source.badges === null}
            onClick={onChangeBadge}
          >
            Change badge
          </ActionButton>
        </div>
      </div>
    </div>
  );
}

function PatreonSection({
  patreon,
  say,
  clear,
}: {
  patreon: Ready["patreon"];
  say: ReturnType<typeof useNotice>["say"];
  clear: () => void;
}): ReactElement {
  const command = useCommand();
  const isLinked =
    patreon.link === "active" ||
    patreon.link === "inactive" ||
    patreon.link === "linked";
  // A link that failed on the way back from Patreon is said once.
  const { problem, dismissProblem } = patreon;
  useEffect(() => {
    if (problem === null) return;
    say(problem);
    dismissProblem();
  }, [problem, dismissProblem, say]);
  const open = isLinked ? patreon.manage : patreon.connect;
  const act = (): void => {
    if (!open) return;
    say("Opening Patreon…", "working");
    void command.run(open).then((outcome) => {
      if (outcome?.status === "fail") say(outcome.message);
      else clear();
    });
  };
  return (
    <>
      <div className={s.provider}>
        <PatreonMark />
        <h3 className={s.heading}>Patreon</h3>
      </div>
      {patreon.link === "unlinked" && <p className={s.quiet}>Not linked.</p>}
      {isLinked && (
        <>
          <p className={s.linkState}>Linked</p>
          {/* Membership is stated only when it is known, either way. */}
          {patreon.link !== "linked" && (
            <p className={s.linkDetail}>
              {patreon.link === "active" ? (
                <>
                  <Check aria-hidden="true" size="1em" strokeWidth={1.6} />
                  Active supporter
                </>
              ) : (
                "No active membership"
              )}
            </p>
          )}
        </>
      )}
      <div className={s.providerAction}>
        <ActionButton
          className={s.action}
          disabled={open === null}
          pending={command.isBusy || patreon.isLinking}
          onClick={act}
        >
          {isLinked ? "Manage on Patreon" : "Connect Patreon"}
        </ActionButton>
      </div>
    </>
  );
}

/** The purchase history: passive rows, with the next page asked for as it nears view. */
function Ledger({ purchases }: { purchases: PurchaseHistory }): ReactElement {
  const scroller = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const { loadMore } = purchases;
  useEffect(() => {
    const root = scroller.current;
    const target = sentinel.current;
    if (!root || !target || !loadMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { root, rootMargin: "120px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [loadMore, purchases.rows.length]);
  if (purchases.rows.length === 0)
    return (
      <div className={s.ledger}>
        {purchases.state === "loading" ? (
          <LoadingState label="Loading purchases">
            <div className={s.skeletonRows}>
              {[0, 1, 2].map((row) => (
                <LoadingShape key={row} width="100%" height="1.1em" />
              ))}
            </div>
          </LoadingState>
        ) : (
          <p className={s.ledgerNote}>
            {/* A list that could not be read is left blank. It is never called empty. */}
            {purchases.state === "ready" ? "No purchases yet" : ""}
          </p>
        )}
      </div>
    );
  return (
    <div
      ref={scroller}
      className={s.ledger}
      role="region"
      aria-label="Purchases"
      tabIndex={0}
    >
      <table className={s.table} role="table">
        <thead role="rowgroup">
          <tr role="row">
            <th scope="col" role="columnheader">
              Item
            </th>
            <th scope="col" role="columnheader">
              Date
            </th>
            <th scope="col" role="columnheader">
              Amount
            </th>
            <th scope="col" role="columnheader">
              Status
            </th>
          </tr>
        </thead>
        <tbody role="rowgroup">
          {purchases.rows.map((row) => (
            <tr role="row" key={row.id}>
              <td role="cell">{row.item}</td>
              <td role="cell">{purchaseDate(row.date)}</td>
              <td role="cell">{row.amount}</td>
              <td role="cell">{row.status}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div ref={sentinel} className={s.more} />
    </div>
  );
}

function BillingSection({
  billing,
  say,
  clear,
}: {
  billing: Ready["billing"];
  say: ReturnType<typeof useNotice>["say"];
  clear: () => void;
}): ReactElement {
  const command = useCommand();
  const { manage } = billing;
  const hasFailed = billing.purchases.state === "failed";
  useEffect(() => {
    if (hasFailed) say("Couldn’t load purchases.");
  }, [hasFailed, say]);
  return (
    <>
      <div className={s.headingRow}>
        <h3 className={s.heading}>Billing &amp; purchases</h3>
        <ActionButton
          className={s.action}
          disabled={manage === null}
          pending={command.isBusy}
          onClick={() => {
            if (!manage) return;
            say("Opening billing…", "working");
            void command.run(manage).then((outcome) => {
              if (outcome?.status === "fail") say(outcome.message);
              else clear();
            });
          }}
        >
          Manage billing
        </ActionButton>
      </div>
      <h4 className={s.subheading}>Purchases</h4>
      <Ledger purchases={billing.purchases} />
    </>
  );
}

function AccountSectionPanel({
  deletion,
  onDelete,
  onClose,
}: {
  deletion: Ready["deletion"];
  onDelete: () => void;
  onClose: () => void;
}): ReactElement {
  const close = useRef<HTMLButtonElement>(null);
  const isRequested = deletion.state === "requested";
  // The confirmation that led here has closed, so focus moves to the one action left.
  useEffect(() => {
    if (isRequested) close.current?.focus({ preventScroll: true });
  }, [isRequested]);
  if (isRequested)
    return (
      <>
        <h3 className={`${s.heading} ${s.requested}`}>Deletion requested</h3>
        <p className={s.quiet}>Waiting for committed activity to finish.</p>
        <div className={s.footer}>
          <ActionButton
            ref={close}
            className={s.action}
            data-autofocus=""
            onClick={onClose}
          >
            Close
          </ActionButton>
        </div>
      </>
    );
  return (
    <>
      <div className={s.headingRow}>
        <h3 className={s.heading}>Account</h3>
      </div>
      <div className={s.settingRow}>
        <p className={s.text}>Delete account</p>
        <ActionButton className={s.dangerAction} onClick={onDelete}>
          Delete account
        </ActionButton>
      </div>
    </>
  );
}

/** Confirms deletion. Closing it any other way does nothing. */
function DeleteConfirmation({
  request,
  onClose,
}: {
  request: Ready["deletion"]["request"];
  onClose: () => void;
}): ReactElement {
  const command = useCommand();
  const notice = useNotice();
  return (
    <Dialog
      open
      title="Delete your account?"
      busy={command.isBusy}
      onClose={onClose}
      actions={
        <>
          <ActionButton
            className={s.action}
            data-autofocus=""
            disabled={command.isBusy}
            onClick={onClose}
          >
            Cancel
          </ActionButton>
          <ActionButton
            className={`${s.action} ${s.dangerConfirm}`}
            pending={command.isBusy}
            onClick={() =>
              void command.run(request).then((outcome) => {
                if (outcome?.status === "success") onClose();
                else if (outcome) notice.say(outcome.message);
              })
            }
          >
            Delete account
          </ActionButton>
        </>
      }
    >
      <p className={s.text}>
        This cannot be undone. Your cards, packs, and coins will be removed.
      </p>
      <div className={s.childNotice}>
        <FeedbackMessages
          label="Deletion messages"
          presentation="toast"
          items={notice.items}
        />
      </div>
    </Dialog>
  );
}

/**
 * Account settings: Profile, Patreon, Billing & purchases and Account. Each
 * change saves by itself from its own small dialog, so there is no Save here.
 * Closing returns to where the person was.
 */
export function AccountSettingsDialog({
  source,
  section,
  onSectionChange,
  onClose,
  returnFocusRef,
}: {
  source: AccountSettingsSource;
  section: AccountSection;
  onSectionChange: (section: AccountSection) => void;
  onClose: () => void;
  /** The control to return focus to on closing, when the opener is gone. */
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const id = useId();
  const notice = useNotice();
  const [child, setChild] = useState<"picture" | "badge" | "delete" | null>(
    null,
  );
  const ready = source.status === "ready" ? source : null;
  const isDeleting = ready?.deletion.state === "requested";
  const current = isDeleting ? "account" : section;
  const isUnavailable =
    source.status !== "ready" && source.status !== "loading";
  const { say, clear } = notice;
  useEffect(() => {
    if (isUnavailable) say("Couldn’t load account.");
  }, [isUnavailable, say]);

  let panel: ReactNode;
  if (!ready)
    panel =
      source.status === "loading" ? (
        <LoadingState label="Loading account">
          <div className={s.person}>
            <LoadingShape shape="portrait" width="196px" height="196px" />
            <LoadingShape width="320px" height="44px" />
          </div>
        </LoadingState>
      ) : null;
  else if (current === "profile")
    panel = (
      <ProfileSection
        source={ready}
        onChangePicture={() => setChild("picture")}
        onChangeBadge={() => setChild("badge")}
      />
    );
  else if (current === "patreon")
    panel = <PatreonSection patreon={ready.patreon} say={say} clear={clear} />;
  else if (current === "billing")
    panel = <BillingSection billing={ready.billing} say={say} clear={clear} />;
  else
    panel = (
      <AccountSectionPanel
        deletion={ready.deletion}
        onDelete={() => setChild("delete")}
        onClose={onClose}
      />
    );

  return (
    <Dialog
      open
      title="Account settings"
      size="wide"
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
      stacked={
        <>
          {ready?.picture && child === "picture" && (
            <PictureEditor
              picture={ready.picture}
              onClose={() => setChild(null)}
            />
          )}
          {ready?.badges && child === "badge" && (
            <BadgeEditor
              portrait={ready.portrait}
              badge={ready.badge}
              badges={ready.badges}
              onClose={() => setChild(null)}
            />
          )}
          {ready && child === "delete" && !isDeleting && (
            <DeleteConfirmation
              request={ready.deletion.request}
              onClose={() => setChild(null)}
            />
          )}
        </>
      }
    >
      <div className={s.layout} data-account-settings={current}>
        <div className={s.columns}>
          <SectionTabs
            id={id}
            label="Account settings"
            sections={ACCOUNT_SECTIONS}
            labels={SECTION_LABEL}
            current={current}
            // Once deletion is requested, only Account stays available.
            isDisabled={(next) => (isDeleting || !ready) && next !== "account"}
            onChange={onSectionChange}
          />
          <div className={s.divider} />
          <div
            role="tabpanel"
            id={`${id}-panel`}
            aria-labelledby={`${id}-tab-${current}`}
            className={s.panel}
          >
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={`${current}:${isDeleting ? "requested" : source.status}`}
                style={{
                  flex: 1,
                  minHeight: 0,
                  display: "flex",
                  flexDirection: "column",
                }}
                initial={{ opacity: 0, y: m.travel / 3 }}
                animate={{
                  opacity: 1,
                  y: 0,
                  transition: {
                    duration: m.duration.settle,
                    ease: m.ease.arrive,
                  },
                }}
                exit={{
                  opacity: 0,
                  transition: {
                    duration: m.duration.feedback / 2,
                    ease: m.ease.depart,
                  },
                }}
              >
                {panel}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
        <div className={s.notice}>
          <FeedbackMessages
            label="Account settings messages"
            presentation="toast"
            items={notice.items}
          />
        </div>
      </div>
    </Dialog>
  );
}
