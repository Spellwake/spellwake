import { style, globalStyle } from "@vanilla-extract/css";
import { vars } from "@spellwake/ui";
export const page = style({
  maxWidth: 1100,
  margin: "0 auto",
  padding: `${vars.space.lg} ${vars.space.lg} ${vars.space.xl}`,
});
export const stage = style({
  height: "min(58svh,580px)",
  minHeight: 320,
  width: "100%",
  borderRadius: vars.radius.panel,
  background: vars.color.raised,
  overflow: "hidden",
});
globalStyle(`${stage} canvas`, {
  display: "block",
  width: "100%",
  height: "100%",
  touchAction: "none",
});
export const actions = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.md,
  flexWrap: "wrap",
  paddingTop: vars.space.lg,
});
export const folds = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: vars.space.xl,
  margin: `${vars.space.lg} 0 0`,
  padding: `${vars.space.md} 0 0`,
  border: 0,
  minWidth: 0,
  "@media": {
    "(max-width: 600px)": { gridTemplateColumns: "1fr", gap: vars.space.lg },
  },
});
export const fold = style({ display: "grid", gap: vars.space.sm, minWidth: 0 });
globalStyle(`${fold} label`, {
  display: "flex",
  justifyContent: "space-between",
  gap: vars.space.md,
});
globalStyle(`${fold} span`, { color: vars.color.muted });
export const rear = style({ color: "#e4b66c" });
export const front = style({ color: "#83d9e4" });
export const note = style({
  color: vars.color.muted,
  marginTop: vars.space.lg,
});
