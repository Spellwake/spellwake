import { style, globalStyle } from "@vanilla-extract/css";
export const assignment = style({
  minHeight: "100svh",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  padding: "9.3svh 14px 5svh",
  margin: "0",
  boxSizing: "border-box",
  color: "#f0e4cd",
  fontFamily: '"Source Serif 4", Georgia, serif',
  textShadow: "0 2px 4px #080a08",
});
globalStyle(`${assignment} header`, { textAlign: "center" });
globalStyle(`${assignment} .combat-progress`, {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "1rem",
  font: "italic clamp(16px, 2.8svh, 30px)/1.3 Georgia, serif",
  margin: "0 0 1.1svh",
});
globalStyle(`${assignment} .combat-progress::before`, {
  content: '""',
  width: "min(12vw, 190px)",
  height: "1px",
  background: "linear-gradient(90deg, transparent, #b7aa91)",
});
globalStyle(`${assignment} .combat-progress::after`, {
  content: '""',
  width: "min(12vw, 190px)",
  height: "1px",
  background: "linear-gradient(90deg, transparent, #b7aa91)",
});
globalStyle(`${assignment} .combat-progress::after`, {
  transform: "rotate(180deg)",
});
globalStyle(`${assignment} h1`, {
  margin: "0",
  fontWeight: "500",
  fontSize: "clamp(25px, min(4.3vw, 5.5svh), 62px)",
  lineHeight: "1.13",
  letterSpacing: "-0.025em",
  outline: "none",
});
globalStyle(`${assignment} .combat-blockers`, {
  display: "flex",
  gap: "min(3.3vw, 5.3svh)",
  marginTop: "4.5svh",
});
globalStyle(`${assignment} .combat-card`, {
  width: "min(42vw, 37.6svh, 445px)",
  position: "relative",
  containerType: "inline-size",
});
globalStyle(`${assignment} .combat-counter`, {
  position: "absolute",
  top: "38.4%",
  left: "50%",
  transform: "translate(-50%, -50%)",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "1px",
  fontVariantNumeric: "tabular-nums",
});
globalStyle(`${assignment} .combat-counter button`, {
  display: "grid",
  placeItems: "center",
  padding: "0",
  color: "#fff2cf",
  background: "linear-gradient(160deg, #3b3527, #15140f 68%, #292419)",
  border: "1.5px solid #c3a66c",
  boxShadow: "0 1px 6px #04040399, inset 0 1px 1px #fff5",
});
globalStyle(`${assignment} .combat-counter output`, {
  display: "grid",
  placeItems: "center",
  padding: "0",
  color: "#fff2cf",
  background: "linear-gradient(160deg, #3b3527, #15140f 68%, #292419)",
  border: "1.5px solid #c3a66c",
  boxShadow: "0 1px 6px #04040399, inset 0 1px 1px #fff5",
});
globalStyle(`${assignment} .combat-counter button`, {
  width: "max(33px, 16cqw)",
  height: "max(24px, 10.7cqw)",
  borderRadius: "3.6cqw",
  cursor: "pointer",
});
globalStyle(`${assignment} .combat-counter button svg`, {
  width: "53%",
  height: "90%",
  strokeWidth: "2.8",
});
globalStyle(`${assignment} .combat-counter output`, {
  width: "max(45px, 21cqw)",
  height: "max(38px, 18.4cqw)",
  borderRadius: "3.7cqw",
  background: "linear-gradient(145deg, #30291f, #120f0b)",
  borderColor: "#9f8e74",
  font: '650 max(26px, 12.5cqw)/1 "Source Serif 4", Georgia, serif',
});
globalStyle(`${assignment} .combat-counter button:disabled`, {
  color: "#9c9485",
  borderColor: "#6e6452",
  cursor: "default",
});
globalStyle(`${assignment} footer`, {
  position: "relative",
  marginTop: "5.9svh",
});
globalStyle(`${assignment} .combat-remaining`, {
  position: "absolute",
  bottom: "calc(100% + 5px)",
  textAlign: "center",
  width: "100%",
  whiteSpace: "nowrap",
});
globalStyle(`${assignment} .combat-buttons`, {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
});
globalStyle(`${assignment} .combat-primary`, {
  cursor: "pointer",
  fontFamily: "inherit",
  color: "#f7e9c6",
});
globalStyle(`${assignment} .combat-back`, {
  cursor: "pointer",
  fontFamily: "inherit",
  color: "#f7e9c6",
});
globalStyle(`${assignment} .combat-primary`, {
  minWidth: "min(48vw, 32svh)",
  padding: "0.23em 1em 0.29em",
  fontWeight: "550",
  fontSize: "clamp(23px, 3.75svh, 43px)",
  lineHeight: "1.25",
  background: "linear-gradient(180deg, #413a27, #242219 53%, #2d291c)",
  border: "2px solid #c7a765",
  borderRadius: "999px",
  boxShadow: "inset 0 1px 1px #fff8, 0 0 16px #a97b2a32",
  textShadow: "0 2px 3px #100d08",
});
globalStyle(`${assignment} .combat-primary:disabled`, {
  opacity: "0.55",
  cursor: "default",
});
globalStyle(`${assignment} .combat-back`, {
  display: "inline-flex",
  alignItems: "center",
  gap: "0.2em",
  minHeight: "40px",
  padding: "4px 8px",
  fontSize: "clamp(17px, 2.25svh, 25px)",
  background: "#161a14c9",
  border: "1px solid #706650",
  borderRadius: "999px",
});
globalStyle(`${assignment} .combat-buttons .combat-back`, {
  position: "absolute",
  right: "calc(100% + min(2vw, 24px))",
});
globalStyle(`${assignment} button:focus-visible`, {
  outline: "3px solid white",
  outlineOffset: "3px",
});
globalStyle(`${assignment} button:enabled:hover`, {
  filter: "brightness(1.2)",
});
globalStyle(`${assignment} button:enabled:active`, {
  transform: "translateY(1px)",
});
globalStyle(`${assignment} .combat-blockers`, {
  maxWidth: "100%",
  overflowX: "auto",
  padding: "3px",
  marginBottom: "-3px",
  marginTop: "calc(4.5svh - 3px)",
});
globalStyle(`${assignment} .combat-card`, { flexShrink: 0 });
globalStyle(assignment, {
  "@media": { "(max-aspect-ratio: 1/1)": { justifyContent: "center" } },
});
globalStyle(`${assignment} .combat-buttons .combat-back`, {
  "@media": {
    "(max-aspect-ratio: 1/1)": { position: "static", marginRight: 10 },
  },
});
