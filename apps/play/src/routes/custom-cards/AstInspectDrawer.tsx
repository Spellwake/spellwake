import type { ReactElement } from "react";
import { Undo2, X } from "lucide-react";
import type {
  CustomCardCompileResult,
  PhrasingChange,
} from "@spellwake/application-contracts";
import { manaSymbolDataUrl } from "@spellwake/ui";
import * as s from "./custom-cards-studio.css";

export interface AstInspectDrawerProps {
  open: boolean;
  report: CustomCardCompileResult | null;
  lastChanges: PhrasingChange[];
  canUndoPhrasing: boolean;
  onUndoPhrasing: () => void;
  onClose: () => void;
}

export function AstInspectDrawer({
  open,
  report,
  lastChanges,
  canUndoPhrasing,
  onUndoPhrasing,
  onClose,
}: AstInspectDrawerProps): ReactElement | null {
  if (!open || !report) return null;

  return (
    <div
      className={s.astDrawer}
      role="region"
      aria-label="Spellwake Engine AST Inspection"
    >
      <div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: "8px",
          }}
        >
          <h3 className={s.astSectionTitle} style={{ margin: 0 }}>
            Verified Engine AST ({report.astReading.length})
          </h3>
          <div style={{ display: "inline-flex", alignItems: "center", gap: "8px", fontSize: "11.5px", color: "#ada598" }}>
            <span>MV {report.manaValue}</span>
            <span>&middot;</span>
            <span className={s.manaCluster}>
              {report.colorIdentity.length > 0 ? (
                report.colorIdentity.map((c) => {
                  const src = manaSymbolDataUrl(`{${c}}`);
                  return src ? (
                    <img key={c} src={src} alt={c} className={s.manaImg} />
                  ) : (
                    <span key={c}>{c}</span>
                  );
                })
              ) : (
                <span>Colorless</span>
              )}
            </span>
          </div>
        </div>

        <div className={s.astList}>
          {report.astReading.length === 0 ? (
            <div className={s.astRow}>
              <span style={{ color: "#9c9487" }}>
                No parsed abilities yet. Add rules text or keywords to see engine lowering.
              </span>
            </div>
          ) : (
            report.astReading.map((item, idx) => {
              const isSimpleKeyword =
                item.kind === "keyword" &&
                item.label.trim().toLowerCase() === item.summary.trim().toLowerCase();
              const isRedundantTitle =
                item.label.trim().toLowerCase() === item.kind.replace("_", " ").toLowerCase();
              return (
                <div key={`${item.faceIndex}-${item.kind}-${idx}`} className={s.astRow}>
                  <span className={s.astKindBadge}>{item.kind.replace("_", " ")}</span>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    {isSimpleKeyword ? (
                      <div style={{ fontWeight: 600, color: "#f6ebd4" }}>{item.label}</div>
                    ) : isRedundantTitle ? (
                      <div style={{ fontSize: "12.5px", color: "#ece3d2", lineHeight: 1.4 }}>
                        {item.summary}
                      </div>
                    ) : (
                      <>
                        <div style={{ fontWeight: 600, color: "#f6ebd4" }}>{item.label}</div>
                        <div style={{ fontSize: "12.5px", color: "#cdc4b4", marginTop: "2px", lineHeight: 1.4 }}>
                          {item.summary}
                        </div>
                      </>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>
      </div>

      <div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: "8px",
          }}
        >
          <h3 className={s.astSectionTitle} style={{ margin: 0 }}>
            Diagnostics &amp; Fingerprint
          </h3>
          <button
            type="button"
            className={s.iconCloseBtn}
            aria-label="Close AST Inspector"
            onClick={onClose}
          >
            <X size={15} />
          </button>
        </div>

        <div className={s.astList}>
          {report.diagnostics.length === 0 ? (
            <div className={s.astRow}>
              <span style={{ color: "#bfe0b0" }}>
                Stage 1 AST and Stage 2 engine lowering passed cleanly.
              </span>
            </div>
          ) : (
            report.diagnostics.map((diag, idx) => (
              <div key={`${diag.code}-${idx}`} className={s.astRow}>
                <span className={s.astKindBadge}>{diag.stage}</span>
                <div style={{ minWidth: 0 }}>
                  <div style={{ color: diag.severity === "error" ? "#f5b8b8" : "#f1d39a" }}>
                    {diag.message}
                  </div>
                </div>
              </div>
            ))
          )}

          {lastChanges.length > 0 && (
            <div className={s.astRow} style={{ justifyContent: "space-between", alignItems: "center" }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontWeight: 600, color: "#e8cb92" }}>
                  Last standardization ({lastChanges.length} edit{lastChanges.length === 1 ? "" : "s"})
                </div>
                <div style={{ fontSize: "11.5px", color: "#b6ae9f" }}>
                  {lastChanges[0]?.reason ?? ""}
                </div>
              </div>
              {canUndoPhrasing && (
                <button type="button" className={s.smallButton} onClick={onUndoPhrasing}>
                  <Undo2 size={13} /> Undo
                </button>
              )}
            </div>
          )}

          <div
            style={{
              fontSize: "11px",
              color: "#857f74",
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
              marginTop: "4px",
            }}
          >
            rules_sha256: {report.rulesSha256.slice(0, 16)}... &middot; {report.compilerRevision}
          </div>
        </div>
      </div>
    </div>
  );
}
