import { style } from "@vanilla-extract/css";
import { controlTokens } from "../controls.css";
export const receipt = style({
  display: "flex",
  flexDirection: "column",
  width: "100%",
  boxSizing: "border-box",
  containerType: "inline-size",
});
export const heading = style({
  display: "flex",
  alignItems: "center",
  gap: 17,
  alignSelf: "flex-start",
  margin: "12px 0 50.3px 60px",
  color: "#f4eee1",
  font: `47px/1.1 ${controlTokens.font.face}`,
  textShadow: "0 2px 3px #000a",
  "@container": {
    "(max-width: 800px)": { margin: "0 0 24px", fontSize: 34, gap: 10 },
  },
});
export const mark = style({
  display: "grid",
  placeItems: "center",
  color: "#abc452",
  filter: "drop-shadow(0 0 6px #9cba3799)",
});
export const trays = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,721fr) minmax(0,725fr)",
  gap: 15,
  gridAutoRows: 587,
  "@container": {
    "(max-width: 800px)": {
      gridTemplateColumns: "minmax(0,1fr)",
      gridAutoRows: "clamp(300px,60vh,530px)",
    },
  },
});
export const done = style({
  alignSelf: "flex-end",
  width: "min(286px,100%)",
  margin: "56px 6px 0 0",
  "@container": { "(max-width: 800px)": { margin: "24px 0 0" } },
});
