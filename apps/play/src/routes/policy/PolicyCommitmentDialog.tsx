import { useId, type ReactElement, type RefObject } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Dialog } from "@spellwake/ui";
import { motionLanguage as m } from "../../lib/motion";
import { SectionTabs } from "../account/SectionTabs";
import * as account from "../account/account.css";
import * as s from "./policy.css";

export const POLICY_SECTIONS = ["agreement", "privacy", "terms"] as const;
export type PolicySection = (typeof POLICY_SECTIONS)[number];
const SECTION_LABEL: Record<PolicySection, string> = {
  agreement: "Community agreement",
  privacy: "Privacy policy",
  terms: "Terms & conditions",
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

function CommunityAgreement(): ReactElement {
  return (
    <>
      <h3 className={account.heading}>Community agreement</h3>
      <p className={s.lead}>
        Spellwake exists so that anyone can play Magic, and to bring back the
        heart and soul of the game, without your wallet deciding what you get to
        play.
      </p>
      <section className={s.part} aria-labelledby="promise-why">
        <h4 id="promise-why" className={s.partHeading}>
          Why we’re doing this differently
        </h4>
        <p className={s.body}>
          Magic is one of the greatest games ever made, and for a lot of people
          around the world it has become wildly unaffordable. In paper, players
          turned to proxies just to keep sitting at the table. Online, digital
          Magic has turned into a cash grab where you keep paying just to play
          the cards you love.
        </p>
        <p className={s.body}>
          It doesn’t have to be that way. The cards already exist, and having a
          place to play them together shouldn’t come with a price tag. Let’s do
          something different.
        </p>
      </section>
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
          Two ways to play, both free
        </h4>
        <p className={s.body}>
          Open Play lets you build and play with any supported card against
          friends or AI bots. Collection Play keeps the heart and soul of a trading
          card game intact: earning coins through play, opening packs from a
          shared finite supply and trading with other players, with no way to
          buy your way ahead. Neither mode is behind a paywall.
        </p>
      </section>
      <section className={s.part} aria-labelledby="promise-support">
        <h4 id="promise-support" className={s.partHeading}>
          Kept running honestly
        </h4>
        <p className={s.body}>
          Every account is ad-free, and we never sell your data. We’d love for
          Spellwake to be sustainable enough to build full-time, and we want to
          earn that honestly: through generous community supporters and optional
          cosmetics for the people who enjoy them. Not by selling cards, selling
          coins or squeezing players.
        </p>
      </section>
      <section className={s.part} aria-labelledby="promise-open">
        <h4 id="promise-open" className={s.partHeading}>
          Built for the community, yours to carry on
        </h4>
        <p className={s.body}>
          We’re committed to making Spellwake’s core software open source.
          Spellwake is for the Magic community. If we ever lose our way or break
          your trust, you should be able to take it, build something better and
          keep these principles alive.
        </p>
      </section>
      <p className={s.pledge}>
        If we break this agreement, we’ll eat our shirt.
      </p>
    </>
  );
}

function PrivacyPolicy(): ReactElement {
  return (
    <>
      <h3 className={account.heading}>Privacy policy</h3>
      <p className={s.lead}>
        We collect only what it takes to run Spellwake, keep accounts safe and
        honor your cosmetics and supporter perks. There are no ads, no ad
        trackers and we never sell your personal information.
      </p>
      <section className={s.part} aria-labelledby="privacy-collect">
        <h4 id="privacy-collect" className={s.partHeading}>
          What we collect
        </h4>
        <ul className={s.promises}>
          <li className={s.promise}>
            <span className={s.itemTitle}>Account and sign-in:</span> Your
            chosen username and the sign-in method you pick (email one-time
            code, Google, Apple or Discord). Spellwake is passwordless and never
            stores a password for your account.
          </li>
          <li className={s.promise}>
            <span className={s.itemTitle}>Gameplay and preferences:</span> Your
            decks, Collection Play inventory, coin balances, match history,
            trades, predictions, friends, blocks, direct messages and cosmetic
            defaults.
          </li>
          <li className={s.promise}>
            <span className={s.itemTitle}>
              Purchases and supporter links (optional):
            </span>{" "}
            Stripe processes individual cosmetic purchases; Spellwake never sees
            or stores your card number, only the transaction and entitlement
            records needed to grant or reverse items. If you link Patreon, we
            store only minimal membership and entitlement identifiers to apply
            your badge, cosmetics and AI bot allowance, never your Patreon name or
            email.
          </li>
          <li className={s.promise}>
            <span className={s.itemTitle}>Technical and security logs:</span>{" "}
            Basic session, error and server logs needed to reconnect matches,
            fix bugs and stop abuse.
          </li>
        </ul>
      </section>
      <section className={s.part} aria-labelledby="privacy-use">
        <h4 id="privacy-use" className={s.partHeading}>
          How we use it
        </h4>
        <p className={s.body}>
          We use this information to run Open Play, Collection Play, trading,
          messaging and hosted AI bot games, verify your sign-in, protect fair play
          and deliver cosmetics or supporter perks. We send only essential
          account messages such as one-time sign-in codes or service notices,
          never marketing spam.
        </p>
      </section>
      <section className={s.part} aria-labelledby="privacy-providers">
        <h4 id="privacy-providers" className={s.partHeading}>
          Services we rely on
        </h4>
        <ul className={s.promises}>
          <li className={s.promise}>
            <span className={s.itemTitle}>
              Authentication, database and hosting:
            </span>{" "}
            Supabase and our hosting infrastructure store and process account
            and game data on our behalf.
          </li>
          <li className={s.promise}>
            <span className={s.itemTitle}>Payments and memberships:</span>{" "}
            Stripe processes cosmetic checkout, and Patreon manages supporter
            memberships and Discord supporter roles.
          </li>
          <li className={s.promise}>
            <span className={s.itemTitle}>Card data:</span> Card imagery and
            game metadata are sourced from Scryfall.
          </li>
        </ul>
      </section>
      <section className={s.part} aria-labelledby="privacy-deletion">
        <h4 id="privacy-deletion" className={s.partHeading}>
          Retention and account deletion
        </h4>
        <p className={s.body}>
          You can request account deletion from Account settings at any time.
          Pending trades, open listings and unfilled orders are canceled, active
          commitments finish settling, and your profile, login credentials and
          local app data are removed. Minimal anonymized or financial records
          (such as completed match records affecting other players, reserved
          usernames and payment or refund references) may be retained as needed
          for game integrity, refund reconciliation and legal obligations.
        </p>
      </section>
      <section className={s.part} aria-labelledby="privacy-choices">
        <h4 id="privacy-choices" className={s.partHeading}>
          Your choices and age minimum
        </h4>
        <p className={s.body}>
          You can change your username, manage cosmetic defaults, unlink Patreon
          or delete your account directly in Spellwake. Spellwake is not
          directed to children under 13 (or the minimum digital-consent age
          where you live), and we remove underage accounts created without
          required consent.
        </p>
      </section>
      <p className={s.draftNote}>
        Draft policy for review. Contact details, effective date and final legal
        review will be completed before public release.
      </p>
    </>
  );
}

function TermsAndConditions(): ReactElement {
  return (
    <>
      <h3 className={account.heading}>Terms &amp; conditions</h3>
      <p className={s.lead}>
        By creating an account or using Spellwake, you agree to play fairly,
        respect other players and follow these terms.
      </p>
      <section className={s.part} aria-labelledby="terms-account">
        <h4 id="terms-account" className={s.partHeading}>
          Your account
        </h4>
        <ul className={s.promises}>
          <li className={s.promise}>
            You must meet the minimum age required to use online services where
            you live (at least 13).
          </li>
          <li className={s.promise}>
            Keep your sign-in method secure and pick a username that is not
            abusive, misleading or impersonating someone else.
          </li>
          <li className={s.promise}>
            Do not use automated bots, exploit bugs or run multiple accounts to
            manipulate Collection Play, trading, markets or AI bot allowances.
          </li>
        </ul>
      </section>
      <section className={s.part} aria-labelledby="terms-virtual">
        <h4 id="terms-virtual" className={s.partHeading}>
          Virtual items, coins and gameplay
        </h4>
        <p className={s.body}>
          In-game coins, Collection Play cards, unopened packs and prediction
          stakes exist only inside Spellwake. They have no real-world monetary
          value, cannot be bought with real money and cannot be sold, cashed out
          or traded outside the app. Formats, seasonal supply rules and daily
          AI-bot allowances may evolve as Spellwake grows.
        </p>
      </section>
      <section className={s.part} aria-labelledby="terms-purchases">
        <h4 id="terms-purchases" className={s.partHeading}>
          Cosmetics and supporter memberships
        </h4>
        <ul className={s.promises}>
          <li className={s.promise}>
            Optional cosmetics are sold individually through Stripe in real
            money, with no premium currency. Optional supporter memberships are
            managed through Patreon.
          </li>
          <li className={s.promise}>
            Cosmetics unlocked through a supporter tier stay in your account
            after normal cancellation. Your supporter badge, Discord role and
            higher AI bot allowance require an active membership.
          </li>
          <li className={s.promise}>
            If a payment is refunded or reversed, the cosmetic grant tied to
            that payment is removed. Refunds follow applicable law and our
            payment providers’ rules.
          </li>
        </ul>
      </section>
      <section className={s.part} aria-labelledby="terms-conduct">
        <h4 id="terms-conduct" className={s.partHeading}>
          Conduct, bans and appeals
        </h4>
        <p className={s.body}>
          Treat other players with respect everywhere on Spellwake, including
          matches, chat, trades, usernames and deck names. We don’t put up with
          harassment, hate speech, cheating or abusing the system.
        </p>
        <p className={s.body}>
          If you cross that line, we reserve the right to restrict or ban your
          account, regardless of your Patreon membership, cosmetic purchases or
          supporter status, with no refund except where required by law. If you
          think a ban was a mistake, you can appeal for review.
        </p>
      </section>
      <section className={s.part} aria-labelledby="terms-liability">
        <h4 id="terms-liability" className={s.partHeading}>
          Availability and liability
        </h4>
        <p className={s.body}>
          Spellwake is provided “as is” and “as available.” We work to keep
          games reliable and data safe, but we cannot guarantee uninterrupted or
          error-free service. To the extent permitted by law, Spellwake and its
          contributors are not liable for indirect or incidental damages, lost
          virtual items or service interruptions.
        </p>
      </section>
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
      <p className={s.draftNote}>
        Draft terms for review. Operator details, governing law, contact
        information and final legal review will be completed before public
        release.
      </p>
    </>
  );
}

/**
 * Policy & Commitment: Spellwake's community agreement, privacy policy, and
 * terms & conditions (including required third-party notices). Opened from the
 * account menu, over whatever screen is open.
 */
export function PolicyCommitmentDialog({
  section,
  onSectionChange,
  onClose,
  returnFocusRef,
}: {
  section: PolicySection;
  onSectionChange: (section: PolicySection) => void;
  onClose: () => void;
  /** The control to return focus to on closing, when the opener is gone. */
  returnFocusRef?: RefObject<HTMLElement | null>;
}): ReactElement {
  const id = useId();
  return (
    <Dialog
      open
      title="Policy & Commitment"
      size="wide"
      onClose={onClose}
      {...(returnFocusRef ? { returnFocusRef } : {})}
    >
      <div className={account.layout} data-policy-commitment={section}>
        <div className={s.columns}>
          <SectionTabs
            id={id}
            label="Policy & Commitment"
            sections={POLICY_SECTIONS}
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
                {section === "agreement" ? (
                  <CommunityAgreement />
                ) : section === "privacy" ? (
                  <PrivacyPolicy />
                ) : (
                  <TermsAndConditions />
                )}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
