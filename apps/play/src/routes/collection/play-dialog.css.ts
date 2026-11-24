import { keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";

/** Concept: the two ways to play a deck, and the search that follows one of them. */
export const choices = style({
  display: "flex",
  flexDirection: "column",
  gap: "12px",
});
export const choice = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "18px",
  minHeight: "88px",
  padding: "14px 18px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.fieldEdge}`,
  background: t.color.field,
  font: "inherit",
  color: t.color.ink,
  textAlign: "left",
  cursor: "pointer",
  selectors: {
    "&:hover": { borderColor: t.color.goldEdge },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
});
export const choiceIcon = style({
  flexShrink: 0,
  width: "30px",
  height: "30px",
  color: t.color.gold,
});
export const choiceText = style({
  display: "flex",
  flexDirection: "column",
  gap: "4px",
  minWidth: 0,
});
export const choiceTitle = style({ fontSize: t.font.small, lineHeight: 1.2 });
export const choiceHint = style({
  fontSize: t.font.hint,
  lineHeight: 1.25,
  color: t.color.muted,
});

export const options = style({
  display: "flex",
  flexDirection: "column",
  gap: "18px",
});

/** A named setting with its switch at the end of the row. */
export const setting = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "16px",
  color: t.color.ink,
});
export const settingText = style({
  display: "flex",
  flexDirection: "column",
  gap: "4px",
  minWidth: 0,
});

export const searching = style({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "18px",
  padding: "18px 0 8px",
});
export const line = style({
  margin: 0,
  fontSize: t.font.small,
  color: t.color.ink,
});
export const elapsed = style({
  fontVariantNumeric: "tabular-nums",
  color: t.color.muted,
});
const breathe = keyframes({
  "0%, 100%": { transform: "scale(0.82)", opacity: 0.55 },
  "50%": { transform: "scale(1)", opacity: 1 },
});
/** A ring that breathes while the search runs and holds full once a match is found. */
export const pulse = style({
  width: "54px",
  height: "54px",
  borderRadius: "50%",
  border: `2px solid ${t.color.gold}`,
  boxShadow: `0 0 22px ${t.color.goldEdge}`,
  selectors: {
    '&[data-found="true"]': { background: t.color.goldEdge },
  },
  "@media": {
    [standardMotion]: {
      selectors: {
        '&[data-found="false"]': {
          animation: `${breathe} 1.8s ease-in-out infinite`,
        },
      },
    },
  },
});
