import { style } from "@vanilla-extract/css";

/* Stand-in table and hand for the match frame probe. */

export const table = style({
  position: "absolute",
  inset: 0,
  background:
    "radial-gradient(ellipse 90% 80% at 50% 45%,#3d4a33,#2c3626 55%,#1a2018 100%)",
});
export const battlefield = style({
  position: "absolute",
  left: "50%",
  top: "50%",
  transform: "translate(-50%,-50%)",
  display: "flex",
  gap: "18px",
});
export const card = style({
  flexShrink: 0,
  width: "118px",
  aspectRatio: "63 / 88",
  borderRadius: "8px",
  background: "linear-gradient(160deg,#f6e9d340,#f6e9d314)",
  border: "1px solid #f6e9d359",
  boxShadow: "0 8px 18px #00000066",
});
export const hand = style({
  height: "100%",
  display: "flex",
  justifyContent: "center",
  alignItems: "flex-start",
  overflow: "hidden",
});
export const handCard = style([
  card,
  { width: "150px", marginInline: "-22px", marginTop: "10px" },
]);
