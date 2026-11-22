import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * Policy & Commitment: reading text in the Account settings layout. Lengths
 * are in the same design-pixel scale as Account settings.
 */
const STACKED = "settings (inline-size < 46rem)";
const rule = "#a698774c";
const sans = '"Albert Sans", system-ui, sans-serif';
const gold = "#e9c983";

/** Slightly wider section rail so longer policy titles stay on one line. */
export const columns = style({
  flex: 1,
  minWidth: 0,
  display: "grid",
  gridTemplateColumns: "332px 1px minmax(0, 1fr)",
  "@container": {
    [STACKED]: {
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateRows: "auto 1px minmax(0, 1fr)",
    },
  },
});

/** The reading column scrolls by itself, so the sections stay in place. */
export const reading = style({
  // A fixed height, so the dialog keeps its size between sections.
  flex: "none",
  height: "min(640px, 62vh)",
  overflowY: "auto",
  paddingRight: "22px",
  scrollbarWidth: "thin",
  scrollbarColor: "#8d8168 transparent",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
  "@container": { [STACKED]: { height: "auto", paddingRight: 0 } },
});

export const lead = style({
  margin: "26px 0 0",
  maxWidth: "34em",
  font: `28px/1.42 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [STACKED]: { fontSize: "24px" } },
});

export const part = style({
  marginTop: "34px",
  paddingTop: "28px",
  borderTop: `1px solid ${rule}`,
});
export const partHeading = style({
  margin: 0,
  font: `31px/1.2 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.active,
  "@container": { [STACKED]: { fontSize: "27px" } },
});
export const body = style({
  margin: "12px 0 0",
  maxWidth: "36em",
  font: `24px/1.5 ${sans}`,
  color: "#e7dccb",
  "@container": { [STACKED]: { fontSize: "21px" } },
});

export const promises = style({
  margin: "16px 0 0",
  padding: 0,
  maxWidth: "36em",
  listStyle: "none",
  display: "grid",
  gap: "12px",
});
export const promise = style({
  position: "relative",
  paddingLeft: "34px",
  font: `24px/1.45 ${sans}`,
  color: "#e7dccb",
  "@container": { [STACKED]: { fontSize: "21px" } },
});
export const itemTitle = style({
  fontWeight: 600,
  color: shellTokens.ink,
});
/** A small gold diamond, the shell's marker, instead of a bullet. */
globalStyle(`${promise}::before`, {
  content: '""',
  position: "absolute",
  left: "6px",
  top: "0.62em",
  width: "9px",
  height: "9px",
  background: gold,
  transform: "rotate(45deg)",
});

/** The founder's pledge, set apart at the end. */
export const pledge = style({
  margin: "40px 0 8px",
  padding: "26px 30px",
  border: "1px solid #b9a67d66",
  borderRadius: "10px",
  background: "linear-gradient(100deg,#37322a99,#2a272266)",
  font: `italic 30px/1.35 ${shellTokens.serif}`,
  color: shellTokens.active,
  "@container": { [STACKED]: { fontSize: "25px", padding: "20px 22px" } },
});

/** A quiet note marking copy that still awaits launch details and legal review. */
export const draftNote = style({
  margin: "34px 0 8px",
  paddingTop: "20px",
  borderTop: `1px solid ${rule}`,
  maxWidth: "36em",
  font: `italic 21px/1.45 ${sans}`,
  color: "#c8bba5",
  "@container": { [STACKED]: { fontSize: "19px" } },
});

/** A notice whose wording is set by someone else, quoted as given. */
export const quoted = style({
  margin: "16px 0 0",
  padding: "20px 24px",
  maxWidth: "36em",
  borderLeft: `3px solid ${gold}`,
  borderRadius: "0 8px 8px 0",
  background: "#0a1016",
  font: `23px/1.5 ${sans}`,
  color: shellTokens.ink,
  "@container": { [STACKED]: { fontSize: "20px" } },
});

export const link = style({
  display: "inline-block",
  marginTop: "14px",
  font: `21px/1.4 ${sans}`,
  color: gold,
  textDecoration: "underline",
  textUnderlineOffset: "4px",
  textDecorationThickness: "1px",
  overflowWrap: "anywhere",
  selectors: {
    "&:hover": { color: shellTokens.active },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "3px",
      borderRadius: "2px",
    },
  },
});
