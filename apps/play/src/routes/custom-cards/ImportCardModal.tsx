import { useEffect, useRef, useState, type ChangeEvent, type DragEvent, type ReactElement } from "react";
import { ScanLine, Upload, X } from "lucide-react";
import type {
  CustomCardCompileResult,
  CustomCardFileV1,
  CustomCardLayout,
  CustomCardRarity,
  VisionImportResult,
} from "@spellwake/application-contracts";
import {
  decodeSwcardJson,
  extractMetadataFromPng,
} from "../../custom-cards/codec";
import {
  prepareVisionOcrUpload,
  processFaceArtFile,
} from "../../custom-cards/art-processor";
import type { StudioDraftFace } from "../../data/demo-custom-cards";
import * as s from "./custom-cards-studio.css";

export interface ImportedStudioCardPayload {
  sourceLabel: string;
  layout: CustomCardLayout;
  rarity: CustomCardRarity;
  collectorNote: string | null;
  faces: StudioDraftFace[];
  compile?: CustomCardCompileResult;
  warnings: string[];
}

export interface ImportCardModalProps {
  open: boolean;
  onClose: () => void;
  onImport: (payload: ImportedStudioCardPayload) => void;
  /** Runs server-side Vision OCR in connected mode, or simulated preview OCR in preview mode. */
  onRunVisionOcr: (input: {
    sourceKind: "external_png" | "external_image";
    imageDataUrl: string;
    fileName: string;
  }) => Promise<VisionImportResult | null>;
  /** Optional preview-only demo state for inspecting Vision OCR loading visuals cleanly. */
  initialPreviewMode?: "idle" | "vision-progress";
}

function fileToDraftPayload(card: CustomCardFileV1, sourceLabel: string): ImportedStudioCardPayload {
  return {
    sourceLabel,
    layout: card.layout,
    rarity: card.rarity,
    collectorNote: card.collectorNote,
    warnings: [],
    faces: card.faces.map((face) => {
      const art = face.art;
      const mimeType = art?.dataUrl.startsWith("data:image/png;")
        ? "image/png"
        : art
          ? "image/webp"
          : null;
      const comma = art ? art.dataUrl.indexOf(",") : -1;
      return {
        name: face.name,
        manaCost: face.manaCost,
        typeLine: face.typeLine,
        oracleText: face.oracleText,
        power: face.power,
        toughness: face.toughness,
        loyalty: face.loyalty,
        defense: face.defense,
        colorIndicator: face.colorIndicator ?? null,
        flavorText: face.flavorText ?? "",
        frameAccent: face.frameAccent ?? "auto",
        artist: art?.artist ?? "",
        artUrl: art?.dataUrl ?? null,
        artMimeType: mimeType,
        artBase64: art && comma >= 0 ? art.dataUrl.slice(comma + 1) : null,
        copyFromRevisionId: null,
        cropX: art?.cropX ?? 0,
        cropY: art?.cropY ?? 0,
        zoom: art?.zoom ?? 1,
      };
    }),
  };
}

export function ImportCardModal({
  open,
  onClose,
  onImport,
  onRunVisionOcr,
  initialPreviewMode = "idle",
}: ImportCardModalProps): ReactElement | null {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [dragging, setDragging] = useState(false);
  const [phase, setPhase] = useState<"idle" | "reading-vision" | "ready" | "error">("idle");
  const [modeKind, setModeKind] = useState<"lossless" | "vision" | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [previewThumb, setPreviewThumb] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [staged, setStaged] = useState<ImportedStudioCardPayload | null>(null);

  useEffect(() => {
    if (!open) {
      setPhase("idle");
      setModeKind(null);
      setFileName(null);
      setPreviewThumb(null);
      setProgress(0);
      setErrorMsg(null);
      setStaged(null);
      return;
    }
    if (initialPreviewMode === "vision-progress") {
      setPhase("reading-vision");
      setModeKind("vision");
      setFileName("external_card.png");
      setProgress(68);
    }
  }, [open, initialPreviewMode]);

  useEffect(() => {
    return () => {
      if (previewThumb?.startsWith("blob:")) {
        URL.revokeObjectURL(previewThumb);
      }
    };
  }, [previewThumb]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  async function processFile(file: File): Promise<void> {
    setErrorMsg(null);
    setStaged(null);
    setFileName(file.name);

    const lower = file.name.toLowerCase();
    const isJson =
      lower.endsWith(".swcard.json") ||
      lower.endsWith(".json") ||
      file.type === "application/json";

    if (isJson) {
      setModeKind("lossless");
      const text = await file.text();
      const decoded = decodeSwcardJson(text);
      if (!decoded.ok) {
        setPhase("error");
        setErrorMsg(decoded.error);
        return;
      }
      const payload = fileToDraftPayload(decoded.card, "Spellwake .swcard.json (lossless)");
      setPreviewThumb(payload.faces[0]?.artUrl ?? null);
      setStaged(payload);
      setPhase("ready");
      return;
    }

    const isRaster =
      file.type === "image/png" ||
      file.type === "image/jpeg" ||
      file.type === "image/webp" ||
      /\.(png|jpe?g|webp)$/i.test(lower);

    if (!isRaster) {
      setPhase("error");
      setErrorMsg(
        "Unsupported file type. Drop a Spellwake PNG, .swcard.json, or a PNG/JPEG/WebP card image.",
      );
      return;
    }

    const bytes = new Uint8Array(await file.arrayBuffer());
    const localThumb = URL.createObjectURL(file);
    setPreviewThumb(localThumb);

    if (file.type === "image/png" || lower.endsWith(".png")) {
      const embedded = extractMetadataFromPng(bytes);
      if (embedded.kind === "embedded") {
        setModeKind("lossless");
        const payload = fileToDraftPayload(
          embedded.card,
          "Spellwake PNG iTXt metadata (lossless)",
        );
        setStaged(payload);
        setPhase("ready");
        return;
      }
      if (embedded.kind === "invalid") {
        setPhase("error");
        setErrorMsg(embedded.error);
        return;
      }
    }

    // Fallback to Vision OCR for external PNG/JPEG/WebP images without Spellwake iTXt metadata.
    setModeKind("vision");
    setPhase("reading-vision");
    setProgress(24);
    try {
      const prepared = await prepareVisionOcrUpload(file);
      setProgress(52);
      const result = await onRunVisionOcr({
        sourceKind:
          file.type === "image/png" || lower.endsWith(".png")
            ? "external_png"
            : "external_image",
        imageDataUrl: `data:${prepared.mimeType};base64,${prepared.base64}`,
        fileName: file.name,
      });
      setProgress(86);
      const extracted = result?.cards[0];
      if (!extracted) {
        setPhase("error");
        setErrorMsg(
          "Vision OCR could not read card fields from this image. Try a clearer crop or import a Spellwake PNG/JSON.",
        );
        return;
      }
      let croppedArt = null;
      try {
        croppedArt = await processFaceArtFile(file, extracted.artCropBox ?? null);
      } catch {
        croppedArt = null;
      }
      setProgress(100);
      const faces: StudioDraftFace[] = extracted.faces.map((face, idx) => ({
        name: face.name,
        manaCost: face.manaCost,
        typeLine: face.typeLine,
        oracleText: face.oracleText,
        power: face.power,
        toughness: face.toughness,
        loyalty: face.loyalty,
        defense: face.defense,
        colorIndicator: face.colorIndicator ?? null,
        flavorText: extracted.flavorTexts[idx] ?? "",
        frameAccent: "auto",
        artist: idx === 0 ? (extracted.artist ?? "") : "",
        artUrl: idx === 0 && croppedArt ? croppedArt.dataUrl : null,
        artMimeType: idx === 0 && croppedArt ? croppedArt.mimeType : null,
        artBase64: idx === 0 && croppedArt ? croppedArt.base64 : null,
        copyFromRevisionId: null,
        cropX: 0,
        cropY: 0,
        zoom: 1,
      }));
      setStaged({
        sourceLabel: "Vision OCR import",
        layout: extracted.layout,
        rarity: extracted.rarity,
        collectorNote: extracted.collectorNote,
        faces,
        compile: extracted.compiled,
        warnings: result.warnings,
      });
      setPhase("ready");
    } catch (err) {
      setPhase("error");
      setErrorMsg(err instanceof Error ? err.message : "Vision OCR import failed.");
    }
  }

  const handleFileChange = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (file) void processFile(file);
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(false);
    const file = event.dataTransfer.files?.[0];
    if (file) void processFile(file);
  };

  return (
    <div
      className={s.modalBackdrop}
      role="dialog"
      aria-modal="true"
      aria-labelledby="import-card-modal-title"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className={s.modalCard}>
        <div className={s.modalHeader}>
          <div>
            <h2 id="import-card-modal-title" className={s.modalTitle}>
              Import Custom Card
            </h2>
            <p className={s.modalSubtitle}>
              Drop a Spellwake PNG, .swcard.json, or any custom card image.
            </p>
          </div>
          <button
            type="button"
            className={s.iconCloseBtn}
            aria-label="Close Import Custom Card"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>

        <input
          ref={inputRef}
          type="file"
          accept=".swcard.json,.json,.png,.jpg,.jpeg,.webp,application/json,image/png,image/jpeg,image/webp"
          style={{ display: "none" }}
          onChange={handleFileChange}
        />

        <div
          className={s.dropzone}
          data-drag={dragging ? "true" : "false"}
          onClick={() => inputRef.current?.click()}
          onDragOver={(event) => {
            event.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={handleDrop}
        >
          {phase === "idle" && (
            <>
              <Upload size={28} color="#d4b57c" />
              <div style={{ fontSize: "14.5px", color: "#f4eee4", fontWeight: 500 }}>
                Drop a card file here or click to browse
              </div>
              <div style={{ fontSize: "12.5px", color: "#9e978a" }}>
                Spellwake PNGs & .swcard.json load instantly. External PNG/JPG/WebP cards use Vision OCR.
              </div>
            </>
          )}

          {phase === "reading-vision" && (
            <div className={s.visionPreviewRow}>
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: "6px",
                }}
              >
                {previewThumb ? (
                  <img
                    src={previewThumb}
                    alt="Uploaded card preview"
                    style={{
                      width: "92px",
                      height: "128px",
                      objectFit: "cover",
                      borderRadius: "8px",
                      border: "1px solid rgba(228, 208, 160, 0.42)",
                      boxShadow: "0 8px 24px rgba(0, 0, 0, 0.45)",
                    }}
                  />
                ) : (
                  <div
                    style={{
                      width: "92px",
                      height: "128px",
                      borderRadius: "8px",
                      border: "1px solid rgba(228, 208, 160, 0.42)",
                      background:
                        "linear-gradient(155deg, #1d3550 0%, #101621 56%, #312117 100%)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      boxShadow: "0 8px 24px rgba(0, 0, 0, 0.45)",
                    }}
                  >
                    <ScanLine size={30} color="#e2c282" />
                  </div>
                )}
                <span style={{ fontSize: "12px", color: "#aba293" }}>
                  {fileName ?? "external_card.png"}
                </span>
              </div>

              <div
                style={{
                  width: "100%",
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: "10px",
                }}
              >
                <div
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "8px",
                    fontSize: "16px",
                    fontWeight: 500,
                    color: "#f8eed9",
                  }}
                >
                  <ScanLine size={18} color="#d4b57c" />
                  <span>Reading card with Vision OCR...</span>
                </div>
                <div className={s.progressTrack}>
                  <div className={s.progressFill} style={{ width: `${progress}%` }} />
                </div>
                <div style={{ fontSize: "13px", color: "#b7ae9f" }}>
                  Cropping artwork and verifying rules against Spellwake&apos;s engine
                </div>
              </div>
            </div>
          )}

          {phase === "ready" && staged && (
            <div className={s.visionPreviewRow}>
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: "6px",
                }}
              >
                {previewThumb ? (
                  <img
                    src={previewThumb}
                    alt={staged.faces[0]?.name || "Imported card"}
                    style={{
                      width: "96px",
                      height: "134px",
                      objectFit: "cover",
                      borderRadius: "8px",
                      border: "1px solid rgba(228, 208, 160, 0.45)",
                    }}
                  />
                ) : (
                  <div
                    style={{
                      width: "96px",
                      height: "134px",
                      borderRadius: "8px",
                      border: "1px solid rgba(228, 208, 160, 0.35)",
                      background: "rgba(25, 32, 44, 0.9)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      fontSize: "12px",
                      color: "#d4b57c",
                    }}
                  >
                    .swcard.json
                  </div>
                )}
                <span style={{ fontSize: "11px", color: "#9e978a" }}>{fileName}</span>
              </div>

              <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
                <div style={{ fontSize: "11px", color: "#d4b57c", textTransform: "uppercase", letterSpacing: "0.06em" }}>
                  {staged.sourceLabel}
                </div>
                <div style={{ fontSize: "18px", fontFamily: 'Baskerville, Georgia, serif', color: "#f8eed9" }}>
                  {staged.faces.map((f) => f.name).join(" // ")}
                </div>
                <div style={{ fontSize: "12.5px", color: "#bfb7a8" }}>
                  {staged.faces[0]?.typeLine} {staged.faces[0]?.manaCost ? `(${staged.faces[0].manaCost})` : ""}
                </div>
                {staged.warnings.length > 0 && (
                  <div style={{ fontSize: "12px", color: "#e8c486", marginTop: "4px" }}>
                    {staged.warnings[0]}
                  </div>
                )}
              </div>
            </div>
          )}

          {phase === "error" && (
            <div style={{ display: "flex", flexDirection: "column", gap: "8px", alignItems: "center" }}>
              <div style={{ fontSize: "14px", color: "#f5b8b8", fontWeight: 500 }}>
                {errorMsg}
              </div>
              <div style={{ fontSize: "12px", color: "#9e978a" }}>
                Click here to choose another PNG or .swcard.json file
              </div>
            </div>
          )}
        </div>

        <div className={s.modeBadgesRow}>
          <div className={s.modeBadge} data-active={modeKind === "lossless" ? "true" : "false"}>
            Spellwake PNG / JSON &rarr; Instant lossless
          </div>
          <div className={s.modeBadge} data-active={modeKind === "vision" ? "true" : "false"}>
            External PNG / JPG / WebP &rarr; Vision OCR
          </div>
        </div>

        <div className={s.modalFooter}>
          <button type="button" className={s.dockActionBtn} onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className={s.goldCtaBtn}
            disabled={!staged || phase !== "ready"}
            onClick={() => {
              if (staged) {
                onImport(staged);
                onClose();
              }
            }}
          >
            Import to Studio
          </button>
        </div>
      </div>
    </div>
  );
}
