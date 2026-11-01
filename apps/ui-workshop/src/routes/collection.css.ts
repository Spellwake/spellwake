import { globalStyle, style } from "@vanilla-extract/css";
import { vars } from "@spellwake/ui/theme";

export const scene = style({
  minHeight: "100svh",
  paddingBottom: 16,
  position: "relative",
  isolation: "isolate",
  backgroundImage: `linear-gradient(90deg,rgba(5,8,14,.55),rgba(5,8,14,.35)),url('${import.meta.env.BASE_URL}backdrop.webp')`,
  backgroundSize: "cover",
  backgroundPosition: "center",
  backgroundAttachment: "fixed",
});
export const shell = style({
  display: "flex",
  minHeight: "calc(100svh - 80px)",
  "@media": { "(max-width: 900px)": { flexDirection: "column" } },
});
export const main = style({
  flex: 1,
  minWidth: 0,
  padding: "36px clamp(24px,3.4vw,58px) 76px",
  "@media": { "(max-width: 640px)": { padding: "24px 16px 90px" } },
});
export const toolbar = style({
  display: "grid",
  gridTemplateColumns:
    "minmax(220px,1.4fr) minmax(180px,1fr) minmax(400px,1.6fr)",
  gap: 26,
  alignItems: "center",
  marginBottom: 24,
  "@media": {
    "(max-width: 1250px)": {
      gridTemplateColumns: "minmax(160px,1fr) minmax(150px,.7fr)",
      gap: 14,
    },
    "(max-width: 640px)": {
      gridTemplateColumns: "1fr",
      gap: 12,
      marginBottom: 22,
    },
  },
});
export const toolbarActions = style({
  display: "flex",
  justifyContent: "flex-end",
  gap: 24,
  "@media": {
    "(max-width: 1250px)": { gridColumn: "1 / -1" },
    "(max-width: 640px)": { gap: 12, justifyContent: "stretch" },
  },
});
globalStyle(`${toolbarActions} > button`, {
  "@media": {
    "(max-width: 640px)": { flex: 1, padding: "11px 12px", fontSize: 16 },
  },
});
export const grid = style({
  display: "grid",
  gridTemplateColumns: "repeat(3,minmax(0,1fr))",
  columnGap: 0,
  maxWidth: 1260,
  rowGap: 4,
  "@media": {
    "(max-width: 1150px)": {
      gridTemplateColumns: "repeat(2,minmax(0,1fr))",
      gap: "24px 20px",
    },
    "(max-width: 600px)": { gridTemplateColumns: "minmax(0,1fr)", rowGap: 28 },
  },
});
export const deck = style({ minWidth: 0 });
export const boxButton = style({
  display: "block",
  width: "100%",
  padding: 0,
  border: 0,
  background: "transparent",
  cursor: "pointer",
  borderRadius: 8,
  transition: "filter 180ms ease-out",
  selectors: {
    "&:hover": { filter: "brightness(1.1)" },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 3,
    },
  },
  "@media": { "(prefers-reduced-motion:reduce)": { transition: "none" } },
});
export const boxArt = style({
  display: "block",
  width: "100%",
  aspectRatio: "430 / 300",
  overflow: "visible",
  filter: "drop-shadow(0 14px 12px rgba(0,0,0,.5))",
});
export const deckInfo = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 16,
  padding: "0 32px 10px",
  marginTop: -36,
  "@media": {
    "(max-width: 1250px)": { padding: "0 10px 10px", marginTop: -16 },
  },
});
export const deckTitle = style({ minWidth: 0 });
globalStyle(`${deckTitle} h2`, {
  fontSize: "clamp(21px,1.6vw,27px)",
  lineHeight: 1.25,
  fontWeight: 500,
  margin: "0 0 4px",
  overflowWrap: "anywhere",
  textShadow: "0 2px 8px #000",
});
globalStyle(`${deckTitle} p`, {
  fontFamily: vars.font.display,
  fontSize: 18,
  lineHeight: 1.4,
  margin: 0,
  color: vars.color.muted,
  textShadow: "0 2px 6px #000",
});
export const empty = style({
  textAlign: "center",
  padding: "80px 24px",
  background: "rgba(8,12,18,.75)",
  borderRadius: 12,
});
globalStyle(`${empty} p`, { marginInline: "auto" });
export const srOnly = style({
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
  border: 0,
});
export const studyTools = style({
  position: "relative",
  width: "fit-content",
  maxWidth: "calc(100% - 32px)",
  margin: "0 16px 0 auto",
  display: "flex",
  alignItems: "center",
  gap: 16,
  padding: "8px 12px",
  borderRadius: 8,
  background: "rgba(7,11,18,.92)",
  border: `1px solid ${vars.color.line}`,
  boxShadow: "0 4px 20px #0005",
  flexWrap: "wrap",
});
globalStyle(`${studyTools} a`, {
  display: "inline-flex",
  alignItems: "center",
  gap: 7,
  fontSize: 12,
  color: vars.color.muted,
  textDecoration: "none",
});
export const referenceFrame = style({
  minHeight: "100svh",
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "center",
  background: "#080c12",
});
export const reference = style({
  display: "block",
  width: "100%",
  height: "auto",
});
export const fieldLabel = style({
  display: "block",
  fontSize: 14,
  marginBottom: 8,
});

// Composition dimensions follow the selected reference; behavior stays shared.
globalStyle(`${scene} > header`, { minHeight: 86 });
globalStyle(`${shell} > nav`, {
  width: 214,
  "@media": { "(max-width:900px)": { width: "100%" } },
});

globalStyle(`${deck}:nth-child(n+4) ${deckInfo}`, { marginTop: 0 });

globalStyle(`${shell} > nav > button`, {
  "@media": {
    "(max-width:640px)": { padding: "12px 14px", gap: 8, minWidth: 0 },
  },
});
