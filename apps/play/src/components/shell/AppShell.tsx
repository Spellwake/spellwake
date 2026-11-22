import { useEffect, useRef, useState, type ReactElement } from "react";
import { FeedbackMessages, OverlayPresentation } from "@spellwake/ui";
import { useLocation, useOutlet } from "react-router-dom";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { backdropArt } from "../../lib/backdrop-art";
import {
  useDeviceClass,
  useOrientation,
  useShellLayout,
} from "../../lib/display-scale";
import { motionLanguage as m } from "../../lib/motion";
import { useFrameMode } from "../../routes/frame-mode";
import {
  NativeAuthReturn,
  useIsAccountRestricted,
  useIsEnteringAccount,
} from "../../routes/entry/EntryRoutes";
import { destinationFor, travelDirection } from "../../routes/navigation";
import {
  AccountOverlays,
  useOpenAccountSettings,
  useOpenCosmetics,
} from "../../account/AccountOverlays";
import { AppHeader } from "./AppHeader";
import {
  MessagesOverlay,
  useMessagesParam,
} from "../../messaging/MessagesOverlay";
import { useHeaderAccount } from "./header-account";
import { Backdrop } from "./Backdrop";
import { CompactNavigation, CompactSections } from "./CompactNavigation";
import { SectionRail } from "./SectionRail";
import * as s from "./shell.css";

/**
 * Frame shared by every signed-in screen. A route chooses how much of it to
 * keep: browsing screens get the header, sections and destinations; a focused
 * task keeps only the backdrop and brings its own task header; an immersive
 * screen owns the whole viewport.
 *
 * A screen change is one move: the outgoing page leaves quickly and the
 * incoming one arrives from the side its destination sits on.
 */
export function AppShell(): ReactElement {
  const location = useLocation();
  const outlet = useOutlet();
  const routeFrame = useFrameMode();
  // Until the account has settled, the screen is an entry step and shows no navigation.
  const isEntering = useIsEnteringAccount();
  // An account awaiting deletion is held outside the app, with no navigation.
  const isRestricted = useIsAccountRestricted();
  const frame = isEntering || isRestricted ? "focus" : routeFrame;
  const openAccountSettings = useOpenAccountSettings();
  const { openCosmetics, openDefaults } = useOpenCosmetics();
  const accountButton = useRef<HTMLButtonElement>(null);
  const messagesButton = useRef<HTMLButtonElement>(null);
  const [messages, setMessages] = useMessagesParam();
  const destination = destinationFor(location.pathname);
  const previous = useRef(location.pathname);
  const direction = travelDirection(previous.current, location.pathname);
  previous.current = location.pathname;
  const isCompact = useShellLayout() === "compact";
  // On a phone every dialog rises from the bottom edge as a drawer.
  const overlays = useDeviceClass() === "phone" ? "drawer" : "dialog";
  // A phone on its side keeps its sections in the header to save a row of height.
  const sectionPlacement =
    useOrientation() === "landscape" ? "inline" : "below";
  const hasChrome = frame === "browse";
  const hasRail =
    hasChrome && !isCompact && (destination?.sections.length ?? 0) > 0;
  const hasDock = hasChrome && isCompact;
  const account = useHeaderAccount();
  const [hasSignOutFailed, setHasSignOutFailed] = useState(false);
  useEffect(() => {
    if (!hasSignOutFailed) return;
    const timer = window.setTimeout(() => setHasSignOutFailed(false), 4000);
    return () => window.clearTimeout(timer);
  }, [hasSignOutFailed]);
  // An accepted sign-out needs nothing more here: the account gate sees the
  // session end and shows sign-in.
  const signOut = account.signOut;
  const onSignOut = signOut
    ? (): void =>
        void signOut().then((isSignedOut) => {
          if (!isSignedOut) setHasSignOutFailed(true);
        })
    : null;
  return (
    <MotionConfig reducedMotion="user">
      <OverlayPresentation value={overlays}>
        <div className={s.scene} data-frame-mode={frame}>
          <NativeAuthReturn />
          {frame !== "immersive" && <Backdrop art={backdropArt} />}
          {hasChrome && (
            <motion.div
              initial={{ opacity: 0, y: -m.travel }}
              animate={{ opacity: 1, y: 0 }}
              transition={{
                duration: m.duration.scene,
                ease: m.ease.arrive,
                delay: 0.25,
              }}
              style={{ position: "relative", zIndex: 2 }}
            >
              <AppHeader
                current={destination}
                coins={account.coins}
                username={account.username}
                identity={account.identity}
                accountRef={accountButton}
                onViewProfile={openAccountSettings}
                onOpenCosmetics={openCosmetics}
                onOpenDefaults={openDefaults}
                onToggleMessages={() =>
                  void setMessages(messages ? null : "open")
                }
                isMessagesOpen={!!messages}
                messagesRef={messagesButton}
                onSignOut={onSignOut}
                hasDestinations={!isCompact}
              >
                {isCompact && sectionPlacement === "inline" && (
                  <CompactSections current={destination} placement="inline" />
                )}
              </AppHeader>
              {isCompact && sectionPlacement === "below" && (
                <CompactSections current={destination} placement="below" />
              )}
            </motion.div>
          )}
          <div className={s.workspace}>
            {/*
              One rail stays mounted while any destination has one: it slides
              in only when it first appears. Between destinations its sections
              change in place with a short fade, so two rails never share the
              width and nothing beside them jumps.
            */}
            <AnimatePresence initial={false}>
              {hasRail && destination && (
                <motion.div
                  key="rail"
                  className={s.railSlot}
                  data-frame="rail"
                  initial={{ opacity: 0, x: -m.travel }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{
                    opacity: 0,
                    transition: { duration: m.duration.feedback },
                  }}
                  transition={{
                    duration: m.duration.scene,
                    ease: m.ease.arrive,
                  }}
                >
                  <AnimatePresence initial={false} mode="wait">
                    <motion.div
                      key={destination.id}
                      className={s.railSections}
                      initial={{ opacity: 0 }}
                      animate={{ opacity: 1 }}
                      exit={{ opacity: 0 }}
                      transition={{
                        duration: m.duration.feedback,
                        ease: m.ease.arrive,
                      }}
                    >
                      <SectionRail destination={destination} />
                    </motion.div>
                  </AnimatePresence>
                </motion.div>
              )}
            </AnimatePresence>
            <main
              className={hasDock ? `${s.stage} ${s.stageBesideDock}` : s.stage}
            >
              <AnimatePresence initial={false} custom={direction}>
                <motion.div
                  key={location.pathname}
                  className={s.page}
                  custom={direction}
                  variants={{
                    enter: (from: number) => ({
                      opacity: 0,
                      x: from * m.travel,
                      y: from === 0 ? m.travel / 2 : 0,
                    }),
                    rest: {
                      opacity: 1,
                      x: 0,
                      y: 0,
                      transition: {
                        duration: m.duration.scene,
                        ease: m.ease.arrive,
                        delay: m.duration.feedback,
                      },
                    },
                    leave: (from: number) => ({
                      opacity: 0,
                      x: from * -m.travel * 0.5,
                      transition: {
                        duration: m.duration.feedback,
                        ease: m.ease.depart,
                      },
                    }),
                  }}
                  initial="enter"
                  animate="rest"
                  exit="leave"
                >
                  {outlet}
                </motion.div>
              </AnimatePresence>
            </main>
          </div>
          {hasDock && <CompactNavigation current={destination} />}
          {/* Account settings and profiles belong to the signed-in app, not to account entry. */}
          {!isEntering && <AccountOverlays accountButton={accountButton} />}
          {!isEntering && <MessagesOverlay trigger={messagesButton} />}
          <FeedbackMessages
            label="Account messages"
            presentation="toast"
            items={
              hasSignOutFailed
                ? [
                    {
                      id: "sign-out-failed",
                      text: "Couldn’t sign out.",
                      onDismiss: () => setHasSignOutFailed(false),
                    },
                  ]
                : []
            }
          />
        </div>
      </OverlayPresentation>
    </MotionConfig>
  );
}
