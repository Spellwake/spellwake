import { style } from "@vanilla-extract/css";
export const prompt = style({
  display: "flex",
  justifyContent: "center",
  alignItems: "baseline",
  gap: 12,
  whiteSpace: "nowrap",
  color: "#f6f0dc",
  font: '400 26px/1.2 Baskerville,"Times New Roman",serif',
  letterSpacing: ".022em",
  textShadow: "0 2px 3px #111a0e, 0 0 2px #152111",
  fontVariantNumeric: "tabular-nums",
  pointerEvents: "none",
});
export const option = style({
  appearance: "none",
  position: "relative",
  display: "block",
  width: "100%",
  height: "100%",
  padding: 0,
  margin: 0,
  border: 0,
  borderRadius: 6,
  background: "transparent",
  cursor: "pointer",
  color: "inherit",
  selectors: {
    '&[data-shape="portrait"]': { borderRadius: "50%" },
    "&:disabled": { cursor: "default" },
    "&:focus-visible": { outline: "3px solid white", outlineOffset: 4 },
  },
});
