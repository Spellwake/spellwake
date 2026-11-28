import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type ReactElement,
} from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  Bot,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Download,
  FileJson,
  FlipHorizontal2,
  ImagePlus,
  Layers,
  LoaderCircle,
  Plus,
  RotateCcw,
  Save,
  Search,
  Sparkles,
  Undo2,
  Upload,
} from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import {
  CUSTOM_CARD_COLORS,
  CUSTOM_CARD_FRAME_ACCENTS,
  CUSTOM_CARD_LAYOUTS,
  CUSTOM_CARD_RARITIES,
  TWO_FACE_CUSTOM_CARD_LAYOUTS,
  type CustomCardColor,
  type CustomCardCompileInput,
  type CustomCardCompileResult,
  type CustomCardFaceRules,
  type CustomCardFileV1,
  type CustomCardFrameAccent,
  type CustomCardLayout,
  type CustomCardRarity,
  type CustomCardSaveArtItem,
  type PhrasingChange,
  type StandardizeResult,
  type VisionImportResult,
} from "@spellwake/application-contracts";
import {
  CustomCardFrame,
  SettingSwitch,
  manaSymbolDataUrl,
  parseManaTokens,
  renderCardFrameToPngBytes,
  type CardFaceFrameInput,
} from "@spellwake/ui";
import {
  embedMetadataInPng,
  encodeSwcardJson,
} from "../../custom-cards/codec";
import { processFaceArtFile } from "../../custom-cards/art-processor";
import { useAuthenticatedCustomArt } from "../../data/custom-art-cache";
import {
  createEmptyDraftFace,
  type DemoVaultCard,
  type StudioDraftFace,
} from "../../data/demo-custom-cards";
import { AstInspectDrawer } from "./AstInspectDrawer";
import {
  ImportCardModal,
  type ImportedStudioCardPayload,
} from "./ImportCardModal";
import { PatreonSupportModal } from "./PatreonSupportModal";
import * as s from "./custom-cards-studio.css";

const LAYOUT_LABELS: Record<CustomCardLayout, string> = {
  normal: "Normal",
  transform: "Transform",
  modal_dfc: "Modal DFC",
  adventure: "Adventure",
  split: "Split",
  planeswalker: "Planeswalker",
  saga: "Saga",
};

const RARITY_LABELS: Record<CustomCardRarity, string> = {
  common: "Common",
  uncommon: "Uncommon",
  rare: "Rare",
  mythic: "Mythic",
};

const ACCENT_LABELS: Record<CustomCardFrameAccent, string> = {
  auto: "Auto",
  W: "White",
  U: "Blue",
  B: "Black",
  R: "Red",
  G: "Green",
  gold: "Gold",
  artifact: "Artifact",
  colorless: "Colorless",
  land: "Land",
};

const COLOR_PIP_BG: Record<CustomCardColor, string> = {
  W: "#efe7d1",
  U: "#488ec9",
  B: "#6e657a",
  R: "#cf593b",
  G: "#43945f",
};

const QUICK_MANA_TOKENS = ["{W}", "{U}", "{B}", "{R}", "{G}", "{C}", "{X}"] as const;

export interface CustomCardsStudioSource {
  mode: "connected" | "preview";
  apiOrigin: string;
  bearer: string | null;
  isPatron: boolean;
  cards: DemoVaultCard[];
  activeCount: number;
  activeLimit: number;
  isLoading: boolean;
  compile: (input: CustomCardCompileInput) => Promise<CustomCardCompileResult | null>;
  standardize: (faces: CustomCardFaceRules[]) => Promise<StandardizeResult | null>;
  saveCard: (input: {
    cardId: string | null;
    expectedRevisionId: string | null;
    layout: CustomCardLayout;
    rarity: CustomCardRarity;
    faces: CustomCardFaceRules[];
    visualFrameAccents: CustomCardFrameAccent[];
    flavorTexts: Array<string | null>;
    collectorNote: string | null;
    art: CustomCardSaveArtItem[];
  }) => Promise<{
    ok: true;
    cardId: string;
    revisionId: string;
    compile: CustomCardCompileResult;
  } | { ok: false; message: string }>;
  archiveCard: (cardId: string, archived: boolean) => Promise<boolean>;
  deleteCard: (cardId: string) => Promise<"deleted" | "archived" | null>;
  importVision: (input: {
    sourceKind: "external_png" | "external_image";
    imageDataUrl: string;
    fileName: string;
  }) => Promise<VisionImportResult | null>;
  quickTest: (
    revisionId: string,
    aiPlaysCustomCard: boolean,
  ) => Promise<{ ok: true; roomId: string } | { ok: false; message: string }>;
}

interface StudioDraft {
  cardId: string | null;
  revisionId: string | null;
  layout: CustomCardLayout;
  rarity: CustomCardRarity;
  collectorNote: string;
  faces: StudioDraftFace[];
}

function summaryToDraft(
  card: DemoVaultCard,
  apiOrigin: string,
): StudioDraft {
  const rev = card.currentRevision;
  const faces: StudioDraftFace[] = rev.faces.map((face, idx) => {
    const artMeta = rev.art.find((a) => a.faceIndex === idx);
    const demoUrl = card.demoArtUrls?.[idx as 0 | 1] ?? null;
    const remotePath =
      !demoUrl && artMeta?.hasArt && apiOrigin
        ? `/v1/custom-cards/revisions/${rev.revisionId}/art/${idx}`
        : null;
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
      flavorText: rev.flavorTexts[idx] ?? "",
      frameAccent: rev.visualFrameAccents[idx] ?? "auto",
      artist: artMeta?.artist ?? "",
      artUrl: demoUrl ?? remotePath,
      artMimeType: artMeta?.mimeType ?? null,
      artBase64: null,
      copyFromRevisionId: artMeta?.hasArt ? rev.revisionId : null,
      cropX: artMeta?.cropX ?? 0,
      cropY: artMeta?.cropY ?? 0,
      zoom: artMeta?.zoom ?? 1,
    };
  });
  return {
    cardId: card.id,
    revisionId: rev.revisionId,
    layout: rev.layout,
    rarity: rev.rarity,
    collectorNote: rev.collectorNote ?? "",
    faces,
  };
}

function toFaceRules(face: StudioDraftFace): CustomCardFaceRules {
  const clean = (v: string | null): string | null =>
    v !== null && v.trim().length > 0 ? v.trim() : null;
  return {
    name: face.name.trim() || "Untitled Card",
    manaCost: clean(face.manaCost),
    typeLine: face.typeLine.trim() || "Creature",
    oracleText: face.oracleText,
    power: clean(face.power),
    toughness: clean(face.toughness),
    loyalty: clean(face.loyalty),
    defense: clean(face.defense),
    ...(face.colorIndicator && face.colorIndicator.length > 0
      ? { colorIndicator: face.colorIndicator }
      : {}),
  };
}

function slugFileName(raw: string): string {
  const clean = raw
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return clean || "custom-card";
}

async function saveBinaryFile(
  bytes: Uint8Array,
  suggestedName: string,
  mimeType: string,
  filterName: string,
  extensions: string[],
): Promise<boolean> {
  if (isTauri()) {
    const [{ save: saveDialog }, { writeFile: tauriWriteFile }] =
      await Promise.all([
        import("@tauri-apps/plugin-dialog"),
        import("@tauri-apps/plugin-fs"),
      ]);
    const dest = await saveDialog({
      defaultPath: suggestedName,
      filters: [{ name: filterName, extensions }],
    });
    if (!dest) return false;
    await tauriWriteFile(dest, bytes);
    return true;
  }
  const blobBytes = new Uint8Array(bytes.byteLength);
  blobBytes.set(bytes);
  const blob = new Blob([blobBytes], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = suggestedName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}

function VaultManaIcons({ manaCost }: { manaCost: string | null }): ReactElement {
  const tokens = parseManaTokens(manaCost);
  if (tokens.length === 0) return <span className={s.manaCluster} />;
  return (
    <span className={s.manaCluster}>
      {tokens.slice(0, 6).map((tok, i) => {
        const src = manaSymbolDataUrl(tok);
        return src ? (
          <img key={`${tok}-${i}`} src={src} alt={tok} className={s.manaImg} />
        ) : (
          <span key={`${tok}-${i}`} style={{ fontSize: "11px", color: "#c8bfb0" }}>
            {tok}
          </span>
        );
      })}
    </span>
  );
}

export function CustomCardsStudioPage({
  source,
}: {
  source: CustomCardsStudioSource;
}): ReactElement {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const artFileInputRef = useRef<HTMLInputElement | null>(null);

  const [searchQuery, setSearchQuery] = useState("");
  const [showArchived, setShowArchived] = useState(false);
  const [activeFaceIndex, setActiveFaceIndex] = useState<0 | 1>(0);
  const [metaOpen, setMetaOpen] = useState(false);
  const [astOpen, setAstOpen] = useState(
    () => source.mode === "preview" && searchParams.get("ast") === "1",
  );
  const [importOpen, setImportOpen] = useState(
    () => source.mode === "preview" && searchParams.get("modal") === "import",
  );
  const [patreonOpen, setPatreonOpen] = useState(
    () => source.mode === "preview" && searchParams.get("modal") === "patreon",
  );
  const [playPopoverOpen, setPlayPopoverOpen] = useState(false);
  const [aiPlaysCustom, setAiPlaysCustom] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const initialCard = useMemo(() => {
    if (source.cards.length === 0) return null;
    const requested = searchParams.get("card");
    if (requested) {
      const matched = source.cards.find(
        (c) =>
          c.id === requested ||
          c.currentRevision.name.toLowerCase().includes(requested.toLowerCase()),
      );
      if (matched) return matched;
    }
    return source.cards[0];
  }, [source.cards, searchParams]);

  const [draft, setDraft] = useState<StudioDraft>(() =>
    initialCard
      ? summaryToDraft(initialCard, source.apiOrigin)
      : {
          cardId: null,
          revisionId: null,
          layout: "normal",
          rarity: "rare",
          collectorNote: "",
          faces: [createEmptyDraftFace("Aetherblade Vanguard")],
        },
  );
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [exporting, setExporting] = useState<"png" | "json" | null>(null);
  const [launchingQuickTest, setLaunchingQuickTest] = useState(false);

  const [compiling, setCompiling] = useState(false);
  const [compileReport, setCompileReport] = useState<CustomCardCompileResult | null>(null);
  const [undoFacesSnapshot, setUndoFacesSnapshot] = useState<StudioDraftFace[] | null>(null);
  const [lastPhrasingChanges, setLastPhrasingChanges] = useState<PhrasingChange[]>([]);
  const [standardizing, setStandardizing] = useState(false);

  const notify = useCallback((message: string) => {
    setToast(message);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  // Select first loaded card when connected vault finishes initial load.
  const seededFromVault = useRef(initialCard !== null);
  useEffect(() => {
    const first = source.cards[0];
    if (!seededFromVault.current && first && !dirty) {
      seededFromVault.current = true;
      setDraft(summaryToDraft(first, source.apiOrigin));
    }
  }, [source.cards, source.apiOrigin, dirty]);

  const isTwoFace = TWO_FACE_CUSTOM_CARD_LAYOUTS.has(draft.layout);
  const clampedFaceIdx: 0 | 1 = isTwoFace && activeFaceIndex === 1 ? 1 : 0;
  const activeFace: StudioDraftFace =
    draft.faces[clampedFaceIdx] ?? draft.faces[0] ?? createEmptyDraftFace();
  const secondaryFace: StudioDraftFace | null = isTwoFace
    ? (draft.faces[clampedFaceIdx === 0 ? 1 : 0] ?? null)
    : null;

  // Resolve authenticated binary art paths cleanly through the ref-counted blob cache.
  const resolvedFrontArt = useAuthenticatedCustomArt(
    source.apiOrigin,
    draft.faces[0]?.artUrl ?? null,
    source.bearer,
  );
  const resolvedBackArt = useAuthenticatedCustomArt(
    source.apiOrigin,
    draft.faces[1]?.artUrl ?? null,
    source.bearer,
  );
  const activeResolvedArt = clampedFaceIdx === 0 ? resolvedFrontArt : resolvedBackArt;
  const secondaryResolvedArt = clampedFaceIdx === 0 ? resolvedBackArt : resolvedFrontArt;

  const compileInput = useMemo<CustomCardCompileInput>(
    () => ({
      layout: draft.layout,
      rarity: draft.rarity,
      faces: draft.faces.slice(0, isTwoFace ? 2 : 1).map(toFaceRules),
    }),
    [draft.layout, draft.rarity, draft.faces, isTwoFace],
  );

  // Debounced live compilation against the authoritative Rust compiler (or offline preview compiler).
  useEffect(() => {
    let cancelled = false;
    setCompiling(true);
    const handle = window.setTimeout(() => {
      void source.compile(compileInput).then((report) => {
        if (!cancelled) {
          setCompileReport(report);
          setCompiling(false);
        }
      });
    }, 220);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [compileInput, source]);

  const updateDraft = useCallback((updater: (prev: StudioDraft) => StudioDraft) => {
    setDraft(updater);
    setDirty(true);
  }, []);

  const updateActiveFace = useCallback(
    (patch: Partial<StudioDraftFace>) => {
      updateDraft((prev) => {
        const faces = [...prev.faces];
        const idx =
          TWO_FACE_CUSTOM_CARD_LAYOUTS.has(prev.layout) && activeFaceIndex === 1 ? 1 : 0;
        const current = faces[idx] ?? createEmptyDraftFace();
        faces[idx] = { ...current, ...patch };
        return { ...prev, faces };
      });
    },
    [activeFaceIndex, updateDraft],
  );

  const handleLayoutChange = (nextLayout: CustomCardLayout) => {
    const needsTwo = TWO_FACE_CUSTOM_CARD_LAYOUTS.has(nextLayout);
    updateDraft((prev) => {
      let faces = [...prev.faces];
      const firstFace = faces[0] ?? createEmptyDraftFace();
      if (needsTwo && faces.length < 2) {
        faces.push({
          ...createEmptyDraftFace(`${firstFace.name || "Card"} (Back)`),
          manaCost: nextLayout === "transform" ? null : "{2}{U}",
          typeLine:
            nextLayout === "adventure"
              ? "Instant \u2014 Adventure"
              : nextLayout === "split"
                ? "Sorcery"
                : "Creature \u2014 Spirit",
          oracleText:
            nextLayout === "adventure"
              ? "Draw a card."
              : "Flying",
          colorIndicator: nextLayout === "transform" ? ["B"] : null,
        });
      } else if (!needsTwo && faces.length > 1) {
        faces = [firstFace];
      }
      const primary = faces[0] ?? firstFace;
      if (nextLayout === "planeswalker" && !primary.typeLine.toLowerCase().includes("planeswalker")) {
        faces[0] = {
          ...primary,
          typeLine: "Legendary Planeswalker \u2014 Hero",
          loyalty: primary.loyalty ?? "4",
          power: null,
          toughness: null,
        };
      }
      if (nextLayout === "saga" && !primary.typeLine.toLowerCase().includes("saga")) {
        faces[0] = {
          ...primary,
          typeLine: "Enchantment \u2014 Saga",
          power: null,
          toughness: null,
          loyalty: null,
        };
      }
      return { ...prev, layout: nextLayout, faces };
    });
    if (!needsTwo) setActiveFaceIndex(0);
  };

  const handleSelectVaultCard = (card: DemoVaultCard) => {
    setDraft(summaryToDraft(card, source.apiOrigin));
    setActiveFaceIndex(0);
    setDirty(false);
    setUndoFacesSnapshot(null);
    setLastPhrasingChanges([]);
    setPlayPopoverOpen(false);
  };

  const handleCreateNewCard = () => {
    setDraft({
      cardId: null,
      revisionId: null,
      layout: "normal",
      rarity: "rare",
      collectorNote: "",
      faces: [createEmptyDraftFace("Untitled Creation")],
    });
    setActiveFaceIndex(0);
    setDirty(true);
    setUndoFacesSnapshot(null);
    setLastPhrasingChanges([]);
    setPlayPopoverOpen(false);
  };

  const handleDuplicateCard = (card: DemoVaultCard) => {
    const cloned = summaryToDraft(card, source.apiOrigin);
    cloned.cardId = null;
    cloned.revisionId = null;
    cloned.faces = cloned.faces.map((f, i) => ({
      ...f,
      name: i === 0 ? `${f.name} Copy` : f.name,
      copyFromRevisionId: null,
    }));
    setDraft(cloned);
    setActiveFaceIndex(0);
    setDirty(true);
    notify("Duplicated into a new draft. Click Save to store it in your vault.");
  };

  const handleStandardize = async () => {
    if (standardizing) return;
    setStandardizing(true);
    try {
      const priorSnapshot = draft.faces.map((f) => ({ ...f }));
      const result = await source.standardize(
        draft.faces.slice(0, isTwoFace ? 2 : 1).map(toFaceRules),
      );
      if (!result) {
        notify("Could not standardize phrasing right now.");
        return;
      }
      if (!result.changed) {
        notify("Card phrasing already follows modern Spellwake Oracle wording.");
        return;
      }
      setUndoFacesSnapshot(priorSnapshot);
      setLastPhrasingChanges(result.changes);
      updateDraft((prev) => ({
        ...prev,
        faces: prev.faces.map((face, idx) => {
          const nextRules = result.faces[idx];
          if (!nextRules) return face;
          return {
            ...face,
            name: nextRules.name,
            manaCost: nextRules.manaCost,
            typeLine: nextRules.typeLine,
            oracleText: nextRules.oracleText,
            power: nextRules.power,
            toughness: nextRules.toughness,
            loyalty: nextRules.loyalty,
            defense: nextRules.defense,
          };
        }),
      }));
      notify(`Standardized ${result.changes.length} phrasing item${result.changes.length === 1 ? "" : "s"}.`);
    } finally {
      setStandardizing(false);
    }
  };

  const handleUndoPhrasing = () => {
    if (!undoFacesSnapshot) return;
    updateDraft((prev) => ({ ...prev, faces: undoFacesSnapshot }));
    setUndoFacesSnapshot(null);
    setLastPhrasingChanges([]);
    notify("Reverted phrasing standardization.");
  };

  const handleArtFileSelect = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      const processed = await processFaceArtFile(file);
      updateActiveFace({
        artUrl: processed.dataUrl,
        artMimeType: processed.mimeType,
        artBase64: processed.base64,
        copyFromRevisionId: null,
        cropX: 0,
        cropY: 0,
        zoom: 1,
      });
      notify(
        `Artwork ready (${Math.max(1, Math.round(processed.byteLength / 1024))} KiB ${processed.mimeType === "image/webp" ? "WebP" : "PNG"}).`,
      );
    } catch (err) {
      notify(err instanceof Error ? err.message : "Could not process artwork.");
    }
  };

  const handleSave = useCallback(async (): Promise<string | null> => {
    if (saving) return draft.revisionId;
    setSaving(true);
    try {
      const activeFaces = draft.faces.slice(0, isTwoFace ? 2 : 1);
      const artItems: CustomCardSaveArtItem[] = [];
      activeFaces.forEach((face, idx) => {
        if (face.artBase64 && face.artMimeType) {
          artItems.push({
            faceIndex: idx,
            mimeType: face.artMimeType,
            artBase64: face.artBase64,
            cropX: face.cropX,
            cropY: face.cropY,
            zoom: face.zoom,
            artist: face.artist.trim() || null,
          });
        } else if (face.copyFromRevisionId && draft.cardId) {
          artItems.push({
            faceIndex: idx,
            copyFromRevisionId: face.copyFromRevisionId,
            cropX: face.cropX,
            cropY: face.cropY,
            zoom: face.zoom,
            artist: face.artist.trim() || null,
          });
        }
      });

      const res = await source.saveCard({
        cardId: draft.cardId,
        expectedRevisionId: draft.revisionId,
        layout: draft.layout,
        rarity: draft.rarity,
        faces: activeFaces.map(toFaceRules),
        visualFrameAccents: activeFaces.map((f) => f.frameAccent),
        flavorTexts: activeFaces.map((f) => (f.flavorText.trim() ? f.flavorText.trim() : null)),
        collectorNote: draft.collectorNote.trim() ? draft.collectorNote.trim() : null,
        art: artItems,
      });
      if (!res.ok) {
        notify(res.message);
        return null;
      }
      setDraft((prev) => ({
        ...prev,
        cardId: res.cardId,
        revisionId: res.revisionId,
        faces: prev.faces.map((f) => ({
          ...f,
          copyFromRevisionId:
            f.artBase64 || f.copyFromRevisionId ? res.revisionId : null,
          artBase64: null,
        })),
      }));
      setCompileReport(res.compile);
      setDirty(false);
      notify("Saved custom card to your Open Play vault.");
      return res.revisionId;
    } finally {
      setSaving(false);
    }
  }, [draft, isTwoFace, notify, saving, source]);

  const frameInput = useMemo<CardFaceFrameInput>(() => {
    const isSingleCardTwoFace =
      draft.layout === "split" || draft.layout === "adventure";
    const primaryFace = isSingleCardTwoFace
      ? (draft.faces[0] ?? activeFace)
      : activeFace;
    const primaryArt = isSingleCardTwoFace
      ? resolvedFrontArt
      : activeResolvedArt;
    const auxFace = isSingleCardTwoFace
      ? (draft.faces[1] ?? null)
      : secondaryFace;
    const auxArt = isSingleCardTwoFace ? resolvedBackArt : secondaryResolvedArt;

    return {
      kind: "card_face",
      layout: draft.layout,
      rarity: draft.rarity,
      frameAccent: primaryFace.frameAccent,
      faceIndex: isSingleCardTwoFace ? 0 : clampedFaceIdx,
      collectorNote: draft.collectorNote.trim() || null,
      face: {
        name: primaryFace.name,
        manaCost: primaryFace.manaCost,
        typeLine: primaryFace.typeLine,
        oracleText: primaryFace.oracleText,
        flavorText: primaryFace.flavorText.trim() || null,
        power: primaryFace.power,
        toughness: primaryFace.toughness,
        loyalty: primaryFace.loyalty,
        defense: primaryFace.defense,
        colorIndicator: primaryFace.colorIndicator,
        artist: primaryFace.artist.trim() || null,
        artUrl: primaryArt,
        cropX: primaryFace.cropX,
        cropY: primaryFace.cropY,
        zoom: primaryFace.zoom,
      },
      secondaryFace: auxFace
        ? {
            name: auxFace.name,
            manaCost: auxFace.manaCost,
            typeLine: auxFace.typeLine,
            oracleText: auxFace.oracleText,
            flavorText: auxFace.flavorText.trim() || null,
            power: auxFace.power,
            toughness: auxFace.toughness,
            loyalty: auxFace.loyalty,
            defense: auxFace.defense,
            colorIndicator: auxFace.colorIndicator,
            artist: auxFace.artist.trim() || null,
            artUrl: auxArt,
            cropX: auxFace.cropX,
            cropY: auxFace.cropY,
            zoom: auxFace.zoom,
          }
        : null,
    };
  }, [
    draft.layout,
    draft.rarity,
    draft.collectorNote,
    draft.faces,
    activeFace,
    clampedFaceIdx,
    resolvedFrontArt,
    resolvedBackArt,
    activeResolvedArt,
    secondaryFace,
    secondaryResolvedArt,
  ]);

  const buildPortableFile = async (): Promise<CustomCardFileV1> => {
    const activeFaces = draft.faces.slice(0, isTwoFace ? 2 : 1);
    const faces = await Promise.all(
      activeFaces.map(async (face, idx) => {
        const resolvedUrl = idx === 0 ? resolvedFrontArt : resolvedBackArt;
        let dataUrl: string | null = null;
        if (face.artBase64 && face.artMimeType) {
          dataUrl = `data:${face.artMimeType};base64,${face.artBase64}`;
        } else if (
          resolvedUrl &&
          (resolvedUrl.startsWith("data:image/webp;base64,") ||
            resolvedUrl.startsWith("data:image/png;base64,"))
        ) {
          dataUrl = resolvedUrl;
        } else if (resolvedUrl && resolvedUrl.startsWith("blob:")) {
          try {
            const resp = await fetch(resolvedUrl);
            const blob = await resp.blob();
            const processed = await processFaceArtFile(blob);
            dataUrl = processed.dataUrl;
          } catch {
            dataUrl = null;
          }
        }
        return {
          ...toFaceRules(face),
          flavorText: face.flavorText.trim() || null,
          frameAccent: face.frameAccent,
          art: dataUrl
            ? {
                dataUrl,
                cropX: face.cropX,
                cropY: face.cropY,
                zoom: face.zoom,
                artist: face.artist.trim() || null,
              }
            : null,
        };
      }),
    );
    return {
      schema: "spellwake-custom-card-v1",
      layout: draft.layout,
      rarity: draft.rarity,
      collectorNote: draft.collectorNote.trim() || null,
      faces,
    };
  };

  const handleExportPng = async () => {
    if (exporting) return;
    setExporting("png");
    try {
      const portable = await buildPortableFile();
      const rawPng = await renderCardFrameToPngBytes(frameInput, {
        width: 1062,
        height: 1482,
      });
      const pngWithMeta = embedMetadataInPng(rawPng, portable);
      const saved = await saveBinaryFile(
        pngWithMeta,
        `${slugFileName(activeFace.name)}.png`,
        "image/png",
        "Spellwake Custom Card PNG",
        ["png"],
      );
      if (saved) notify("Exported 300-DPI PNG with embedded Spellwake card metadata.");
    } catch (err) {
      notify(err instanceof Error ? err.message : "Could not export PNG.");
    } finally {
      setExporting(null);
    }
  };

  const handleExportJson = async () => {
    if (exporting) return;
    setExporting("json");
    try {
      const portable = await buildPortableFile();
      const jsonText = encodeSwcardJson(portable);
      const bytes = new TextEncoder().encode(jsonText);
      const saved = await saveBinaryFile(
        bytes,
        `${slugFileName(activeFace.name)}.swcard.json`,
        "application/json",
        "Spellwake Custom Card JSON",
        ["json"],
      );
      if (saved) notify("Exported .swcard.json card file.");
    } catch (err) {
      notify(err instanceof Error ? err.message : "Could not export JSON.");
    } finally {
      setExporting(null);
    }
  };

  const handleImportPayload = (payload: ImportedStudioCardPayload) => {
    setDraft({
      cardId: null,
      revisionId: null,
      layout: payload.layout,
      rarity: payload.rarity,
      collectorNote: payload.collectorNote ?? "",
      faces: payload.faces,
    });
    setActiveFaceIndex(0);
    setDirty(true);
    if (payload.compile) setCompileReport(payload.compile);
    notify(`Imported ${payload.faces[0]?.name || "custom card"} via ${payload.sourceLabel}.`);
  };

  const handlePlayWithCard = () => {
    if (!source.isPatron) {
      setPatreonOpen(true);
      return;
    }
    setPlayPopoverOpen((open) => !open);
  };

  const handleConnectPatreon = async () => {
    if (dirty) {
      await handleSave();
    }
    if (isTauri()) {
      try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl("https://www.patreon.com/spellwake");
      } catch {
        notify("Open https://www.patreon.com/spellwake in your browser to link Patreon.");
      }
      setPatreonOpen(false);
      return;
    }
    setPatreonOpen(false);
    void navigate("/open-play/custom-cards?account=patreon");
  };

  const handleLaunchQuickTest = async () => {
    if (launchingQuickTest) return;
    setLaunchingQuickTest(true);
    try {
      let revisionId = draft.revisionId;
      if (dirty || !revisionId) {
        revisionId = await handleSave();
      }
      if (!revisionId) return;
      const result = await source.quickTest(revisionId, aiPlaysCustom);
      if (!result.ok) {
        notify(result.message);
        return;
      }
      setPlayPopoverOpen(false);
      void navigate(`/open-play/lobby?room=${encodeURIComponent(result.roomId)}`);
    } finally {
      setLaunchingQuickTest(false);
    }
  };

  const filteredCards = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return source.cards.filter((card) => {
      if (!showArchived && card.archivedAt !== null) return false;
      if (!q) return true;
      return card.currentRevision.name.toLowerCase().includes(q);
    });
  }, [source.cards, searchQuery, showArchived]);

  const typeLower = activeFace.typeLine.toLowerCase();
  const showPt = typeLower.includes("creature") || typeLower.includes("vehicle");
  const showLoyalty =
    typeLower.includes("planeswalker") || draft.layout === "planeswalker";
  const showDefense = typeLower.includes("battle");

  const dockStatus: "ready" | "warning" | "error" | "checking" = compiling
    ? "checking"
    : compileReport?.stage2Playable
      ? "ready"
      : compileReport?.diagnostics.some((d) => d.severity === "error")
        ? "error"
        : "warning";

  const statusText =
    dockStatus === "checking"
      ? "Checking..."
      : dockStatus === "ready"
        ? "Ready to play"
        : dockStatus === "error"
          ? "Error in rules"
          : "Needs wording fix";

  const warningDiagnostics = useMemo(
    () =>
      (compileReport?.diagnostics ?? []).filter(
        (d) => d.faceIndex === null || d.faceIndex === clampedFaceIdx,
      ),
    [compileReport, clampedFaceIdx],
  );

  const stage1SuggestionCount = warningDiagnostics.filter(
    (d) => d.stage === "stage1" && d.severity === "warning",
  ).length;

  const dockAstChips = useMemo(() => {
    if (!compileReport) return [];
    const seen = new Set<string>();
    const chips: string[] = [];
    for (const item of compileReport.astReading) {
      if (seen.has(item.label)) continue;
      seen.add(item.label);
      chips.push(item.label);
      if (chips.length >= 4) break;
    }
    return chips;
  }, [compileReport]);

  return (
    <section className={s.page} aria-label="Custom Card Studio">
      {toast && (
        <div className={s.toastBanner} role="status">
          {toast}
        </div>
      )}

      <div className={s.workspace}>
        {/* Left Rail: My Cards Vault */}
        <aside className={s.vaultPanel} aria-label="My Custom Cards Vault">
          <div className={s.vaultHeader}>
            <h1 className={s.panelTitle}>My Cards</h1>
            <div className={s.vaultHeaderActions}>
              <button
                type="button"
                className={s.smallButton}
                onClick={handleCreateNewCard}
              >
                <Plus size={13} /> New
              </button>
              <button
                type="button"
                className={s.smallButton}
                onClick={() => setImportOpen(true)}
              >
                <Upload size={13} /> Import
              </button>
            </div>
          </div>

          <div className={s.vaultMetaRow}>
            <span>
              {source.activeCount} / {source.activeLimit}
            </span>
            <button
              type="button"
              className={s.archiveFilterButton}
              data-active={showArchived ? "true" : "false"}
              onClick={() => setShowArchived((v) => !v)}
            >
              {showArchived ? "Hide archived" : "JSON · PNG"}
            </button>
          </div>

          <div className={s.searchWrap}>
            <Search size={14} className={s.searchIcon} />
            <input
              type="search"
              className={s.searchInput}
              placeholder="Search cards..."
              aria-label="Search custom cards"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
            />
          </div>

          <div className={s.vaultList}>
            {source.isLoading && filteredCards.length === 0 ? (
              <div style={{ padding: "18px 8px", color: "#9e978a", fontSize: "13px" }}>
                Loading custom cards...
              </div>
            ) : filteredCards.length === 0 ? (
              <div style={{ padding: "18px 8px", color: "#9e978a", fontSize: "13px" }}>
                No custom cards match. Click + New or Import a PNG / .swcard.json.
              </div>
            ) : (
              filteredCards.map((card) => {
                const isSelected = draft.cardId === card.id;
                const rev = card.currentRevision;
                const cardStatus: "ready" | "warning" | "error" =
                  isSelected && compileReport && dockStatus !== "checking"
                    ? dockStatus
                    : rev.stage2Playable
                      ? "ready"
                      : rev.stage1Parsed
                        ? "warning"
                        : "error";
                return (
                  <div
                    key={card.id}
                    role="button"
                    tabIndex={0}
                    className={s.vaultCard}
                    data-selected={isSelected ? "true" : "false"}
                    onClick={() => handleSelectVaultCard(card)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        handleSelectVaultCard(card);
                      }
                    }}
                  >
                    {isSelected && <span className={s.vaultCardEyebrow}>Selected</span>}
                    <div className={s.vaultCardRow}>
                      <span className={s.vaultCardName}>{rev.faces[0]?.name || rev.name}</span>
                      <span
                        className={s.statusDot}
                        data-status={cardStatus}
                        title={
                          cardStatus === "ready"
                            ? "Ready to play in Open Play"
                            : "Needs wording review"
                        }
                      />
                    </div>
                    <div className={s.vaultCardSubRow}>
                      <VaultManaIcons manaCost={rev.faces[0]?.manaCost ?? null} />
                      <span style={{ fontSize: "10.5px", color: "#968e80" }}>
                        {rev.faces.length === 2 ? LAYOUT_LABELS[rev.layout] : RARITY_LABELS[rev.rarity]}
                      </span>
                    </div>
                    {isSelected && (
                      <div
                        className={s.vaultCardMenuRow}
                        onClick={(event) => event.stopPropagation()}
                      >
                        <button
                          type="button"
                          className={s.microGhostBtn}
                          onClick={() => handleDuplicateCard(card)}
                        >
                          Duplicate
                        </button>
                        <button
                          type="button"
                          className={s.microGhostBtn}
                          onClick={() => {
                            void source.archiveCard(card.id, card.archivedAt === null).then((ok) => {
                              if (ok) {
                                notify(
                                  card.archivedAt === null
                                    ? "Archived card."
                                    : "Restored card.",
                                );
                              }
                            });
                          }}
                        >
                          {card.archivedAt === null ? "Archive" : "Restore"}
                        </button>
                        <button
                          type="button"
                          className={s.microGhostBtn}
                          data-danger="true"
                          onClick={() => {
                            void source.deleteCard(card.id).then((disp) => {
                              if (disp === "deleted") {
                                notify("Deleted custom card.");
                                handleCreateNewCard();
                              } else if (disp === "archived") {
                                notify("Card is referenced by a deck or match, so it was archived.");
                              }
                            });
                          }}
                        >
                          Delete
                        </button>
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </aside>

        {/* Center Panel: Authoring Form */}
        <div className={s.formPanel} aria-label="Card Authoring Form">
          <div className={s.formHeaderRow}>
            <h2 className={s.panelTitle}>Authoring Form</h2>
            {dirty && <span className={s.dirtyBadge}>Unsaved draft</span>}
          </div>

          <div className={s.rowGrid2}>
            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-card-name">
                Card Name
              </label>
              <input
                id="sw-card-name"
                type="text"
                className={s.inputBase}
                maxLength={120}
                value={activeFace.name}
                onChange={(event) => updateActiveFace({ name: event.target.value })}
                placeholder="Aetherblade Vanguard"
              />
            </div>

            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-mana-cost">
                Mana Cost
              </label>
              <div className={s.manaFieldRow}>
                <input
                  id="sw-mana-cost"
                  type="text"
                  className={s.manaInput}
                  maxLength={128}
                  value={activeFace.manaCost ?? ""}
                  onChange={(event) =>
                    updateActiveFace({ manaCost: event.target.value || null })
                  }
                  placeholder="{2}{U}{R}"
                />
                <div className={s.manaPalette} aria-label="Insert mana symbol">
                  {QUICK_MANA_TOKENS.map((tok) => {
                    const src = manaSymbolDataUrl(tok);
                    return (
                      <button
                        key={tok}
                        type="button"
                        className={s.manaSymbolBtn}
                        title={`Append ${tok}`}
                        onClick={() =>
                          updateActiveFace({
                            manaCost: `${activeFace.manaCost ?? ""}${tok}`,
                          })
                        }
                      >
                        {src ? (
                          <img src={src} alt={tok} width={16} height={16} />
                        ) : (
                          tok
                        )}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          </div>

          <div className={s.rowGridType}>
            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-type-line">
                Type Line
              </label>
              <input
                id="sw-type-line"
                type="text"
                className={s.inputBase}
                maxLength={256}
                value={activeFace.typeLine}
                onChange={(event) => updateActiveFace({ typeLine: event.target.value })}
                placeholder="Creature \u2014 Human Artificer"
              />
            </div>

            {showPt && (
              <div className={s.fieldGroup}>
                <span className={s.fieldLabel}>P / T</span>
                <div className={s.ptRow}>
                  <input
                    type="text"
                    className={s.statInput}
                    aria-label="Power"
                    maxLength={8}
                    value={activeFace.power ?? ""}
                    onChange={(event) =>
                      updateActiveFace({ power: event.target.value || null })
                    }
                    placeholder="3"
                  />
                  <span className={s.statSlash}>/</span>
                  <input
                    type="text"
                    className={s.statInput}
                    aria-label="Toughness"
                    maxLength={8}
                    value={activeFace.toughness ?? ""}
                    onChange={(event) =>
                      updateActiveFace({ toughness: event.target.value || null })
                    }
                    placeholder="2"
                  />
                </div>
              </div>
            )}

            {showLoyalty && !showPt && (
              <div className={s.fieldGroup}>
                <label className={s.fieldLabel} htmlFor="sw-loyalty">
                  Loyalty
                </label>
                <input
                  id="sw-loyalty"
                  type="text"
                  className={s.statInput}
                  maxLength={8}
                  value={activeFace.loyalty ?? ""}
                  onChange={(event) =>
                    updateActiveFace({ loyalty: event.target.value || null })
                  }
                  placeholder="4"
                />
              </div>
            )}

            {showDefense && !showPt && !showLoyalty && (
              <div className={s.fieldGroup}>
                <label className={s.fieldLabel} htmlFor="sw-defense">
                  Defense
                </label>
                <input
                  id="sw-defense"
                  type="text"
                  className={s.statInput}
                  maxLength={8}
                  value={activeFace.defense ?? ""}
                  onChange={(event) =>
                    updateActiveFace({ defense: event.target.value || null })
                  }
                  placeholder="5"
                />
              </div>
            )}
          </div>

          <div className={isTwoFace ? s.rowGrid4 : s.rowGrid3}>
            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-layout">
                Layout
              </label>
              <select
                id="sw-layout"
                className={s.selectBase}
                value={draft.layout}
                onChange={(event) =>
                  handleLayoutChange(event.target.value as CustomCardLayout)
                }
              >
                {CUSTOM_CARD_LAYOUTS.map((layout) => (
                  <option key={layout} value={layout}>
                    {LAYOUT_LABELS[layout]}
                  </option>
                ))}
              </select>
            </div>

            {isTwoFace && (
              <div className={s.fieldGroup}>
                <span className={s.fieldLabel}>Active Face</span>
                <div className={s.segmented} role="tablist" aria-label="Active face">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={clampedFaceIdx === 0}
                    className={s.segmentBtn}
                    data-active={clampedFaceIdx === 0 ? "true" : "false"}
                    onClick={() => setActiveFaceIndex(0)}
                  >
                    {draft.layout === "adventure"
                      ? "Creature"
                      : draft.layout === "split"
                        ? "Left"
                        : "Front"}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={clampedFaceIdx === 1}
                    className={s.segmentBtn}
                    data-active={clampedFaceIdx === 1 ? "true" : "false"}
                    onClick={() => setActiveFaceIndex(1)}
                  >
                    {draft.layout === "adventure"
                      ? "Adventure"
                      : draft.layout === "split"
                        ? "Right"
                        : "Back"}
                  </button>
                </div>
              </div>
            )}

            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-rarity">
                Rarity
              </label>
              <select
                id="sw-rarity"
                className={s.selectBase}
                value={draft.rarity}
                onChange={(event) =>
                  updateDraft((prev) => ({
                    ...prev,
                    rarity: event.target.value as CustomCardRarity,
                  }))
                }
              >
                {CUSTOM_CARD_RARITIES.map((rarity) => (
                  <option key={rarity} value={rarity}>
                    {RARITY_LABELS[rarity]}
                  </option>
                ))}
              </select>
            </div>

            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-accent">
                Frame Accent
              </label>
              <select
                id="sw-accent"
                className={s.selectBase}
                value={activeFace.frameAccent}
                onChange={(event) =>
                  updateActiveFace({
                    frameAccent: event.target.value as CustomCardFrameAccent,
                  })
                }
              >
                {CUSTOM_CARD_FRAME_ACCENTS.map((accent) => (
                  <option key={accent} value={accent}>
                    {ACCENT_LABELS[accent]}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Rules Text with native textarea + crisp diagnostic chips underneath */}
          <div className={s.fieldGroup}>
            <div className={s.fieldLabelRow}>
              <label className={s.fieldLabel} htmlFor="sw-rules-text">
                Rules Text
              </label>
              <div className={s.standardizeGroup}>
                <div className={s.colorIndicatorRow} title="Optional rules-level Color Indicator">
                  {CUSTOM_CARD_COLORS.map((c) => {
                    const active = activeFace.colorIndicator?.includes(c) ?? false;
                    return (
                      <button
                        key={c}
                        type="button"
                        className={s.colorPipBtn}
                        data-active={active ? "true" : "false"}
                        style={active ? { background: COLOR_PIP_BG[c] } : undefined}
                        aria-pressed={active}
                        aria-label={`Color indicator ${c}`}
                        onClick={() => {
                          const current = new Set(activeFace.colorIndicator ?? []);
                          if (current.has(c)) current.delete(c);
                          else current.add(c);
                          const ordered = CUSTOM_CARD_COLORS.filter((x) => current.has(x));
                          updateActiveFace({
                            colorIndicator: ordered.length > 0 ? ordered : null,
                          });
                        }}
                      >
                        {c}
                      </button>
                    );
                  })}
                </div>

                {undoFacesSnapshot && (
                  <button
                    type="button"
                    className={s.smallButton}
                    onClick={handleUndoPhrasing}
                  >
                    <Undo2 size={12} /> Undo phrasing
                  </button>
                )}

                <button
                  type="button"
                  className={s.smallButton}
                  disabled={standardizing}
                  onClick={() => void handleStandardize()}
                >
                  <Sparkles size={12} />
                  {standardizing ? "Standardizing..." : "Standardize phrasing"}
                </button>

                {stage1SuggestionCount > 0 && (
                  <span className={s.suggestionPill}>
                    {stage1SuggestionCount} suggestion
                    {stage1SuggestionCount === 1 ? "" : "s"}
                  </span>
                )}
              </div>
            </div>

            <textarea
              id="sw-rules-text"
              className={s.rulesTextarea}
              maxLength={4096}
              value={activeFace.oracleText}
              onChange={(event) => updateActiveFace({ oracleText: event.target.value })}
              placeholder="Flying, haste&#10;When this creature enters, draw a card."
            />

            {warningDiagnostics.length > 0 && (
              <div className={s.diagnosticsRow}>
                <div className={s.diagnosticChips}>
                  {warningDiagnostics.slice(0, 3).map((diag, idx) => (
                    <span
                      key={`${diag.code}-${idx}`}
                      className={s.diagChip}
                      data-severity={diag.severity}
                    >
                      {diag.message}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>

          <div className={s.bottomFormGrid}>
            <div className={s.fieldGroup}>
              <label className={s.fieldLabel} htmlFor="sw-flavor">
                Flavor Text
              </label>
              <textarea
                id="sw-flavor"
                className={s.flavorTextarea}
                maxLength={512}
                value={activeFace.flavorText}
                onChange={(event) => updateActiveFace({ flavorText: event.target.value })}
                placeholder="Optional italic flavor text..."
              />
            </div>

            <div className={s.fieldGroup}>
              <span className={s.fieldLabel}>Artwork</span>
              <input
                ref={artFileInputRef}
                type="file"
                accept="image/png,image/jpeg,image/webp"
                style={{ display: "none" }}
                onChange={(event) => void handleArtFileSelect(event)}
              />
              <button
                type="button"
                className={s.artworkDropTile}
                onClick={() => artFileInputRef.current?.click()}
              >
                {activeResolvedArt ? (
                  <img
                    src={activeResolvedArt}
                    alt="Current card art"
                    className={s.artworkThumb}
                  />
                ) : (
                  <ImagePlus size={18} color="#d4b57c" />
                )}
                <span>{activeResolvedArt ? "Replace art" : "Upload art"}</span>
              </button>
            </div>
          </div>

          <div className={s.disclosureBox}>
            <button
              type="button"
              className={s.disclosureTrigger}
              aria-expanded={metaOpen}
              onClick={() => setMetaOpen((open) => !open)}
            >
              <span>Artist &amp; Collector Note</span>
              {metaOpen ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
            </button>
            {metaOpen && (
              <div className={s.rowGrid2}>
                <div className={s.fieldGroup}>
                  <label className={s.fieldLabel} htmlFor="sw-artist">
                    Artist Credit
                  </label>
                  <input
                    id="sw-artist"
                    type="text"
                    className={s.inputBase}
                    maxLength={80}
                    value={activeFace.artist}
                    onChange={(event) => updateActiveFace({ artist: event.target.value })}
                    placeholder="Elena Vance"
                  />
                </div>
                <div className={s.fieldGroup}>
                  <label className={s.fieldLabel} htmlFor="sw-collector-note">
                    Collector Note
                  </label>
                  <input
                    id="sw-collector-note"
                    type="text"
                    className={s.inputBase}
                    maxLength={120}
                    value={draft.collectorNote}
                    onChange={(event) =>
                      updateDraft((prev) => ({ ...prev, collectorNote: event.target.value }))
                    }
                    placeholder="014/250"
                  />
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Right Stage: Interactive 300-DPI Live Card Preview */}
        <div className={s.stagePanel} aria-label="Live Card Preview Stage">
          <div className={s.cardViewport}>
            <div className={s.cardHolder}>
              <CustomCardFrame
                input={frameInput}
                artInteractive
                onArtPanDelta={(dx, dy) => {
                  const nextX = Math.max(-1, Math.min(1, activeFace.cropX - dx * 2));
                  const nextY = Math.max(-1, Math.min(1, activeFace.cropY - dy * 2));
                  updateActiveFace({ cropX: nextX, cropY: nextY });
                }}
                onArtZoomWheel={(delta) => {
                  const nextZoom = Math.max(0.5, Math.min(4, Number((activeFace.zoom + delta).toFixed(2))));
                  updateActiveFace({ zoom: nextZoom });
                }}
              />
            </div>
          </div>

          <div className={s.framingBar}>
            <div style={{ display: "inline-flex", alignItems: "center", gap: "6px" }}>
              {isTwoFace && (
                <button
                  type="button"
                  className={s.smallButton}
                  onClick={() => setActiveFaceIndex((idx) => (idx === 0 ? 1 : 0))}
                >
                  <FlipHorizontal2 size={13} /> Front | Back
                </button>
              )}
              <button
                type="button"
                className={s.smallButton}
                title="Reset art pan and zoom"
                onClick={() => updateActiveFace({ cropX: 0, cropY: 0, zoom: 1 })}
              >
                <RotateCcw size={12} /> Reset
              </button>
              <button
                type="button"
                className={s.smallButton}
                onClick={() => artFileInputRef.current?.click()}
              >
                <ImagePlus size={12} /> Art
              </button>
            </div>

            <label className={s.zoomControl}>
              <span>Zoom</span>
              <input
                type="range"
                className={s.zoomSlider}
                aria-label="Art Zoom"
                min={0.5}
                max={4}
                step={0.05}
                value={activeFace.zoom}
                onChange={(event) =>
                  updateActiveFace({ zoom: Number(event.target.value) })
                }
              />
              <span style={{ minWidth: "32px", textAlign: "right" }}>
                {activeFace.zoom.toFixed(1)}x
              </span>
            </label>
          </div>
        </div>
      </div>

      {/* Bottom Action Dock */}
      <div className={s.dockWrap}>
        <AstInspectDrawer
          open={astOpen}
          report={compileReport}
          lastChanges={lastPhrasingChanges}
          canUndoPhrasing={undoFacesSnapshot !== null}
          onUndoPhrasing={handleUndoPhrasing}
          onClose={() => setAstOpen(false)}
        />

        {playPopoverOpen && source.isPatron && (
          <div className={s.playPopover} role="dialog" aria-label="Play with card options">
            <div style={{ fontFamily: 'Baskerville, Georgia, serif', fontSize: "17px", color: "#f6ebd4" }}>
              Play in Open Play
            </div>
            <div style={{ fontSize: "12.5px", color: "#b7afa0", lineHeight: 1.4 }}>
              Launch an instant 1v1 60-card Open Play Quick Test room against a Normal-tier bot.
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: "10px",
                padding: "8px 10px",
                borderRadius: "8px",
                background: "rgba(255, 255, 255, 0.04)",
              }}
            >
              <span style={{ fontSize: "12.5px", color: "#ebe1cf" }}>
                Let bots also play this custom card
              </span>
              <SettingSwitch
                label="Let bots also play this custom card"
                checked={aiPlaysCustom}
                onCheckedChange={setAiPlaysCustom}
              />
            </div>
            <button
              type="button"
              className={s.goldCtaBtn}
              disabled={launchingQuickTest || !compileReport?.stage2Playable}
              onClick={() => void handleLaunchQuickTest()}
            >
              <Bot size={15} />
              {launchingQuickTest ? "Creating Quick Test..." : "Quick Test vs. Bot"}
            </button>
            <button
              type="button"
              className={s.dockActionBtn}
              onClick={() => {
                setPlayPopoverOpen(false);
                void navigate("/open-play/decks");
              }}
            >
              <Layers size={14} /> Add to Open Play Deck
            </button>
          </div>
        )}

        <footer className={s.dock}>
          <div className={s.dockLeft}>
            <span className={s.statusPill} data-status={dockStatus}>
              {dockStatus === "checking" ? (
                <LoaderCircle size={13} className={s.spinnerIcon} />
              ) : (
                <span className={s.statusDot} data-status={dockStatus} />
              )}
              {statusText}
            </span>

            {dockAstChips.map((chip) => (
              <span key={chip} className={s.astChip}>
                {chip}
              </span>
            ))}

            <button
              type="button"
              className={s.inspectBtn}
              aria-expanded={astOpen}
              onClick={() => setAstOpen((open) => !open)}
            >
              Inspect AST
              {astOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </button>
          </div>

          <div className={s.dockRight}>
            <button
              type="button"
              className={s.dockActionBtn}
              disabled={exporting !== null}
              onClick={() => void handleExportPng()}
            >
              <Download size={14} />
              {exporting === "png" ? "PNG..." : "PNG"}
            </button>
            <button
              type="button"
              className={s.dockActionBtn}
              disabled={exporting !== null}
              onClick={() => void handleExportJson()}
            >
              <FileJson size={14} />
              {exporting === "json" ? "JSON..." : "JSON"}
            </button>

            <span className={s.dockDivider} aria-hidden="true" />

            <button
              type="button"
              className={s.dockActionBtn}
              disabled={saving}
              onClick={() => void handleSave()}
            >
              <Save size={14} />
              {saving ? "Saving..." : "Save"}
            </button>

            <button
              type="button"
              className={s.goldCtaBtn}
              onClick={handlePlayWithCard}
            >
              Play with card
            </button>
          </div>
        </footer>
      </div>

      <ImportCardModal
        open={importOpen}
        onClose={() => setImportOpen(false)}
        onImport={handleImportPayload}
        onRunVisionOcr={source.importVision}
        initialPreviewMode={
          source.mode === "preview" && searchParams.get("vision") === "progress"
            ? "vision-progress"
            : "idle"
        }
      />

      <PatreonSupportModal
        open={patreonOpen}
        previewCard={frameInput}
        onClose={() => setPatreonOpen(false)}
        onConnectPatreon={handleConnectPatreon}
      />
    </section>
  );
}
