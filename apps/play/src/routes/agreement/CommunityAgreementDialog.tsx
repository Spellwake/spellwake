import { useId, type ReactElement, type RefObject } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Dialog } from "@spellwake/ui";
import { motionLanguage as m } from "../../lib/motion";
import { SectionTabs } from "../account/SectionTabs";
import * as account from "../account/account.css";
import * as s from "./agreement.css";

export const AGREEMENT_SECTIONS = ["promise", "notices"] as const;
export type AgreementSection = (typeof AGREEMENT_SECTIONS)[number];
const SECTION_LABEL: Record<AgreementSection, string> = {
  promise: "Our promise",
  notices: "Legal notices",
};

const FAN_CONTENT_POLICY =
  "https://company.wizards.com/en/legal/fancontentpolicy";
const SCRYFALL = "https://scryfall.com";

/** A link whose address is its text, so it reads the same where it cannot open. */
function Address({ href }: { href: string }): ReactElement {
  return (
    <a className={s.link} href={href} target="_blank" rel="noreferrer">
      {href.replace(/^https:\/\//, "")}
    </a>
  );
}

function OurPromise(): ReactElement {
  return (
    <>
      <h3 className={account.heading}>Our promise</h3>
      <p className={s.lead}>
        Spellwake exists so that anyone can play Magic, whatever they can
        afford. We want to bring the light back to Magic: the joy of playing,
        collecting and trading, without your wallet deciding what you get to do.
      </p>
      <section className={s.part} aria-labelledby="promise-money">
        <h4 id="promise-money" className={s.partHeading}>
          Money never buys play
        </h4>
        <ul className={s.promises}>
          <li className={s.promise}>
            Coins are never sold, directly or indirectly.
          </li>
          <li className={s.promise}>
            Cards and packs are never sold for real money, and neither are
            better odds or faster collecting.
          </li>
          <li className={s.promise}>
            Paid cosmetics change how things look, never how they play.
          </li>
          <li className={s.promise}>
            Predictions stake and pay in-game coins. There’s no real money and
            no cash-out.
          </li>
        </ul>
      </section>
      <section className={s.part} aria-labelledby="promise-free">
        <h4 id="promise-free" className={s.partHeading}>
          Free, either way you play
        </h4>
        <p className={s.body}>
          Open Play gives you every supported card. Collection Play gives you a
          collection you earn by playing, trading and winning. Neither is behind
          a paywall.
        </p>
      </section>
      <section className={s.part} aria-labelledby="promise-support">
        <h4 id="promise-support" className={s.partHeading}>
          Kept running by the community
        </h4>
        <p className={s.body}>
          Every account is ad-free. Supporters, memberships and cosmetics pay
          for Spellwake’s running and its development. They never buy cards,
          coins or an edge in a game.
        </p>
      </section>
      <section className={s.part} aria-labelledby="promise-open">
        <h4 id="promise-open" className={s.partHeading}>
          Yours to carry on
        </h4>
        <p className={s.body}>
          We’re committed to making Spellwake’s core software open source. If we
          ever lose our way, you should be able to take it, build something
          better and keep these principles going.
        </p>
      </section>
      <p className={s.pledge}>
        If we break this agreement, we’ll eat our shirt.
      </p>
    </>
  );
}

function Notices(): ReactElement {
  return (
    <>
      <h3 className={account.heading}>Legal notices</h3>
      <section className={s.part} aria-labelledby="notice-wizards">
        <h4 id="notice-wizards" className={s.partHeading}>
          Wizards of the Coast
        </h4>
        {/* Wording required by Wizards’ Fan Content Policy. Keep it exact. */}
        <p className={s.quoted}>
          Spellwake is unofficial Fan Content permitted under the Fan Content
          Policy. Not approved/endorsed by Wizards. Portions of the materials
          used are property of Wizards of the Coast. ©Wizards of the Coast LLC.
        </p>
        <Address href={FAN_CONTENT_POLICY} />
      </section>
      <section className={s.part} aria-labelledby="notice-scryfall">
        <h4 id="notice-scryfall" className={s.partHeading}>
          Scryfall
        </h4>
        <p className={s.body}>
          Card images and set and mana symbols come from Scryfall. Spellwake
          isn’t produced or endorsed by Scryfall.
        </p>
        <Address href={SCRYFALL} />
      </section>
    </>
  );
}

/**
 * Community agreement: what Spellwake promises its players, and the notices
 * it owes the people whose work it uses. Opened from the account menu, over
 * whatever screen is open.
 */
export function CommunityAgreementDialog({
  section,
  onSectionChange,
  onClose,
  returnFocusRef,
}: {
  section: AgreementSection;
  onSectionChange: (section: AgreementSection) => void;
  onClose: () => void;
  /** The control to return focus to on closing, when the opener is gone. */
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const id = useId();
  return (
    <Dialog
      open
      title="Community agreement"
      size="wide"
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
    >
      <div className={account.layout} data-community-agreement={section}>
        <div className={account.columns}>
          <SectionTabs
            id={id}
            label="Community agreement"
            sections={AGREEMENT_SECTIONS}
            labels={SECTION_LABEL}
            current={section}
            onChange={onSectionChange}
          />
          <div className={account.divider} />
          <div
            role="tabpanel"
            id={`${id}-panel`}
            aria-labelledby={`${id}-tab-${section}`}
            className={account.panel}
          >
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={section}
                className={s.reading}
                // The reading column scrolls, so it takes keyboard focus.
                tabIndex={0}
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
                {section === "promise" ? <OurPromise /> : <Notices />}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
