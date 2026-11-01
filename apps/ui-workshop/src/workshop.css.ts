import { style, globalStyle, globalFontFace } from "@vanilla-extract/css";
import { vars } from "@spellwake/ui/theme";

for (const [family, file] of [
  ["Source Serif 4", "sourceserif4.woff2"],
  ["Albert Sans", "albert-sans.woff2"],
  ["Libre Baskerville", "libre-baskerville.woff2"],
  ["Lora", "lora.woff2"],
] as const)
  globalFontFace(family, {
    src: `url('${import.meta.env.BASE_URL}fonts/${file}') format('woff2')`,
    fontWeight: "100 900",
    fontStyle: "normal",
    fontDisplay: "swap",
  });
globalStyle("html", { scrollBehavior: "smooth", scrollPaddingTop: 30 });
globalStyle("body", {
  backgroundImage: `linear-gradient(90deg,rgba(5,9,15,.83),rgba(7,11,18,.68)),url('${import.meta.env.BASE_URL}backdrop.webp')`,
  backgroundSize: "cover",
  backgroundAttachment: "fixed",
  backgroundPosition: "center",
});
globalStyle("a", { color: vars.color.gold, textUnderlineOffset: "4px" });
globalStyle("h1,h2,h3,p", { marginTop: 0 });
globalStyle("h1,h2,h3", { fontFamily: vars.font.display, fontWeight: 500 });
globalStyle("h1", {
  fontSize: "clamp(30px,3.2vw,44px)",
  lineHeight: 1.15,
  marginBottom: 12,
  letterSpacing: "-.018em",
});
globalStyle("h2", { fontSize: 29, lineHeight: 1.2, marginBottom: 12 });
globalStyle("h3", { fontSize: 22, lineHeight: 1.3, marginBottom: 12 });
globalStyle("p", {
  color: vars.color.muted,
  lineHeight: 1.7,
  fontSize: 14,
  maxWidth: "72ch",
});
globalStyle("svg", { verticalAlign: "middle" });
export const masthead = style({
  borderBottom: "1px solid rgba(204,190,165,.15)",
  padding: "20px clamp(20px,4vw,64px)",
  display: "flex",
  gap: 24,
  alignItems: "center",
  justifyContent: "space-between",
  flexWrap: "wrap",
  background: "rgba(7,11,18,.8)",
});
export const brand = style({
  fontFamily: vars.font.display,
  fontSize: 23,
  color: "#dcccaf",
  display: "flex",
  alignItems: "center",
  gap: 14,
});
export const tabs = style({
  display: "flex",
  alignItems: "center",
  gap: 6,
  flexWrap: "wrap",
});
export const page = style({
  width: "100%",
  minWidth: 0,
  maxWidth: 1500,
  margin: "0 auto",
  padding: "48px clamp(20px,4vw,64px) 80px",
});
export const intro = style({
  display: "flex",
  alignItems: "flex-end",
  justifyContent: "space-between",
  gap: 32,
  marginBottom: 36,
  flexWrap: "wrap",
});
export const inline = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 12,
});
export const muted = style({ color: vars.color.muted, fontSize: 13 });
export const section = style({
  padding: "36px 0 44px",
  borderTop: "1px solid rgba(204,190,165,.18)",
});
export const sectionHead = style({
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "space-between",
  gap: 24,
  marginBottom: 24,
  flexWrap: "wrap",
});
export const two = style({
  display: "grid",
  gridTemplateColumns: "repeat(2,minmax(0,1fr))",
  gap: 32,
  "@media": { "(max-width: 800px)": { gridTemplateColumns: "1fr", gap: 24 } },
});
export const three = style({
  display: "grid",
  gridTemplateColumns: "repeat(3,minmax(0,1fr))",
  gap: 24,
  "@media": { "(max-width: 900px)": { gridTemplateColumns: "1fr" } },
});
export const group = style({
  minWidth: 0,
  padding: 24,
  borderRadius: 10,
  background: "rgba(7,12,20,.67)",
  "@media": { "(max-width: 640px)": { padding: 18 } },
});
export const surface = style({
  background: "rgba(7,12,20,.74)",
  borderRadius: 10,
  padding: 28,
});
export const horizontalScroll = style({
  width: "100%",
  minWidth: 0,
  overflowX: "auto",
  padding: "5px 4px 14px",
});
export const matrix = style({
  borderCollapse: "collapse",
  width: "100%",
  minWidth: 890,
  textAlign: "left",
});
globalStyle(`${matrix} th`, {
  fontFamily: vars.font.utility,
  color: vars.color.muted,
  fontWeight: 400,
  fontSize: 12,
  padding: "8px 12px 16px",
});
globalStyle(`${matrix} td`, {
  padding: "12px 12px",
  borderTop: "1px solid rgba(210,190,150,.11)",
});
globalStyle(`${matrix} td:first-child`, {
  fontSize: 13,
  color: vars.color.muted,
  width: 110,
});
export const reference = style({
  display: "block",
  width: "100%",
  borderRadius: 8,
});
export const sampleFrame = style({
  borderRadius: 10,
  overflow: "hidden",
  backgroundImage: `linear-gradient(90deg,rgba(4,7,13,.12),rgba(4,7,13,.08)),url('${import.meta.env.BASE_URL}backdrop.webp')`,
  backgroundSize: "cover",
  backgroundPosition: "center",
  boxShadow: "0 14px 40px #0003",
});
export const sampleBody = style({
  display: "flex",
  minHeight: 330,
  "@media": { "(max-width: 900px)": { flexDirection: "column" } },
});
export const sampleContent = style({
  padding: "36px 32px",
  flex: 1,
  minWidth: 0,
  "@media": { "(max-width: 640px)": { padding: "24px 16px" } },
});
export const toolbar = style({
  display: "grid",
  gridTemplateColumns: "minmax(160px,1fr) minmax(150px,210px) auto",
  gap: 16,
  alignItems: "center",
  "@media": {
    "(max-width: 1050px)": { gridTemplateColumns: "1fr 1fr" },
    "(max-width: 500px)": { gridTemplateColumns: "1fr" },
  },
});
export const exampleLabel = style({
  fontSize: 12,
  color: vars.color.muted,
  margin: "0 0 10px",
  display: "block",
});
export const swatches = style({ display: "flex", flexWrap: "wrap", gap: 12 });
export const swatch = style({
  minWidth: 110,
  display: "flex",
  gap: 10,
  alignItems: "center",
  color: vars.color.muted,
  fontSize: 12,
});
export const swatchDot = style({
  width: 28,
  height: 28,
  borderRadius: 5,
  border: "1px solid #ffffff20",
});
export const fontCard = style({
  padding: "22px 0",
  borderBottom: "1px solid rgba(210,190,150,.13)",
});
export const serifSample = style({
  fontSize: 32,
  color: vars.color.text,
  margin: "12px 0 8px",
});
export const form = style({
  display: "flex",
  flexDirection: "column",
  gap: 20,
});
export const status = style({
  position: "fixed",
  bottom: 24,
  left: "50%",
  transform: "translateX(-50%)",
  maxWidth: "calc(100vw - 32px)",
  zIndex: 120,
  background: "#202632",
  color: "#eee0c9",
  padding: "14px 20px",
  borderRadius: 8,
  boxShadow: "0 10px 30px #0005",
  display: "flex",
  alignItems: "center",
  gap: 14,
  fontSize: 14,
});
export const empty = style({
  padding: "36px 20px",
  textAlign: "center",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 10,
  minHeight: 190,
});
export const referenceDialog = style({ maxHeight: "60vh", overflow: "auto" });
export const code = style({
  fontFamily: "ui-monospace,monospace",
  fontSize: 12,
  color: "#c4b49a",
  background: "#10151d",
  padding: "3px 7px",
  borderRadius: 4,
});
export const tiny = style({ fontSize: 12, color: vars.color.muted });
export const cards = style({
  display: "grid",
  gridTemplateColumns: "repeat(2,minmax(0,1fr))",
  gap: 16,
  marginTop: 24,
  "@media": { "(max-width: 600px)": { gridTemplateColumns: "1fr" } },
});
export const deckRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: 16,
  gap: 16,
  background: "linear-gradient(90deg,rgba(7,11,19,.85),rgba(7,11,19,.45))",
  borderRadius: 6,
});

globalStyle(`${toolbar} > :first-child`, {
  "@media": {
    "(max-width: 1050px)": { gridColumn: "1 / -1" },
    "(max-width: 500px)": { gridColumn: "auto" },
  },
});
