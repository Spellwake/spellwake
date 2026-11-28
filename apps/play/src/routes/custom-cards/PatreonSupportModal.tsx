import { useEffect, useState, type ReactElement } from "react";
import { Bot, Link2, Users, X } from "lucide-react";
import { CustomCardFrame, type CardFaceFrameInput } from "@spellwake/ui";
import * as s from "./custom-cards-studio.css";

export interface PatreonSupportModalProps {
  open: boolean;
  previewCard: CardFaceFrameInput;
  onClose: () => void;
  /** Auto-saves any dirty draft first, then initiates Patreon link / opens external browser on Tauri. */
  onConnectPatreon: () => Promise<void>;
}

export function PatreonSupportModal({
  open,
  previewCard,
  onClose,
  onConnectPatreon,
}: PatreonSupportModalProps): ReactElement | null {
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setBusy(false);
      return;
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  return (
    <div
      className={s.modalBackdrop}
      role="dialog"
      aria-modal="true"
      aria-labelledby="patreon-support-modal-title"
      onClick={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div className={s.patreonModalCard}>
        <button
          type="button"
          className={s.iconCloseBtn}
          style={{ position: "absolute", top: "14px", right: "14px" }}
          aria-label="Close Play Custom Cards in Open Play"
          disabled={busy}
          onClick={onClose}
        >
          <X size={18} />
        </button>

        <div
          style={{
            borderRadius: "12px",
            overflow: "hidden",
            boxShadow:
              "0 14px 36px rgba(0, 0, 0, 0.62), 0 0 0 1px rgba(243, 226, 183, 0.18)",
          }}
        >
          <CustomCardFrame input={previewCard} ariaLabel="Custom card preview" />
        </div>

        <div style={{ display: "flex", flexDirection: "column" }}>
          <h2 id="patreon-support-modal-title" className={s.modalTitle}>
            Play Custom Cards in Open Play
          </h2>
          <p className={s.modalSubtitle}>
            Creating, compiling, and exporting cards is free. Support Spellwake on Patreon to test against AI bots and host custom friend tables.
          </p>

          <ul className={s.benefitList}>
            <li className={s.benefitItem}>
              <Bot size={18} className={s.benefitIcon} />
              <span>Quick Test vs. Bot (let the bot pilot your custom card too)</span>
            </li>
            <li className={s.benefitItem}>
              <Users size={18} className={s.benefitIcon} />
              <span>Host custom-card Open Play tables with friends</span>
            </li>
            <li className={s.benefitItem}>
              <Link2 size={18} className={s.benefitIcon} />
              <span>Lend your Open Play decks so non-Patron friends can join</span>
            </li>
          </ul>

          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-end",
              gap: "10px",
            }}
          >
            <button
              type="button"
              className={s.dockActionBtn}
              disabled={busy}
              onClick={onClose}
            >
              Not now
            </button>
            <button
              type="button"
              className={s.goldCtaBtn}
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void onConnectPatreon().finally(() => setBusy(false));
              }}
            >
              {busy ? "Saving draft..." : "Connect Patreon"}
            </button>
          </div>
          <div
            style={{
              textAlign: "right",
              fontSize: "11.5px",
              color: "#968e80",
              marginTop: "7px",
            }}
          >
            Your draft is saved before redirecting
          </div>
        </div>
      </div>
    </div>
  );
}
