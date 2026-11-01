import {
  style,
  styleVariants,
  globalStyle,
  keyframes,
} from "@vanilla-extract/css";
import { vars } from "./theme.css";

const turn = keyframes({ to: { transform: "rotate(360deg)" } });
const enter = keyframes({
  from: { opacity: 0, transform: "translateY(-4px)" },
  to: { opacity: 1, transform: "translateY(0)" },
});
export const control = style({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 9,
  position: "relative",
  border: `1px solid ${vars.color.line}`,
  borderRadius: vars.radius.control,
  background: "rgba(13,17,24,.62)",
  color: vars.color.text,
  fontFamily: vars.font.display,
  fontSize: 18,
  fontWeight: 400,
  lineHeight: 1.15,
  textDecoration: "none",
  whiteSpace: "nowrap",
  cursor: "pointer",
  transition:
    "background-color 140ms ease-out, border-color 140ms ease-out, color 140ms ease-out",
  selectors: {
    "&:hover:not(:disabled), &[data-preview-state=hover]": {
      background: "rgba(44,43,41,.70)",
      borderColor: vars.color.hoverLine,
    },
    "&:active:not(:disabled), &[data-preview-state=pressed]": {
      background: "rgba(61,52,39,.78)",
      borderColor: vars.color.goldMuted,
    },
    "&:focus-visible, &[data-preview-state=focus]": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 3,
    },
    "&:disabled": { opacity: 0.38, cursor: "not-allowed" },
    "&[data-selected=true]": {
      background: "rgba(113,87,48,.27)",
      borderColor: vars.color.goldMuted,
      color: "#f4d8a5",
    },
  },
  "@media": {
    "(prefers-reduced-motion: reduce)": { transition: "none" },
    "(max-width: 640px)": { minHeight: 44 },
  },
});
export const size = styleVariants({
  sm: { minHeight: 38, padding: "8px 14px", fontSize: 16 },
  md: { minHeight: 48, padding: "11px 20px" },
  lg: { minHeight: 56, padding: "14px 26px", fontSize: 20 },
});
export const variant = styleVariants({
  primary: {
    background:
      "linear-gradient(180deg, rgba(101,74,39,.42), rgba(43,32,23,.72))",
    borderColor: vars.color.goldMuted,
    color: "#f7e2bb",
    selectors: {
      "&:hover:not(:disabled), &[data-preview-state=hover]": {
        background: "rgba(104,78,43,.62)",
        borderColor: vars.color.gold,
      },
      "&:active:not(:disabled), &[data-preview-state=pressed]": {
        background: "rgba(61,44,26,.86)",
      },
    },
  },
  secondary: {
    background: "rgba(14,17,23,.58)",
    borderColor: "rgba(180,160,125,.56)",
  },
  ghost: {
    background: "transparent",
    borderColor: "transparent",
    selectors: {
      "&:hover:not(:disabled), &[data-preview-state=hover]": {
        borderColor: "transparent",
        background: "rgba(220,206,176,.09)",
      },
      "&:active:not(:disabled), &[data-preview-state=pressed]": {
        borderColor: "transparent",
        background: "rgba(220,206,176,.14)",
      },
    },
  },
  transparent: {
    background: "transparent",
    borderColor: "transparent",
    color: vars.color.muted,
    selectors: {
      "&:hover:not(:disabled), &:active:not(:disabled), &[data-preview-state=hover], &[data-preview-state=pressed]":
        {
          background: "transparent",
          borderColor: "transparent",
          color: vars.color.text,
        },
    },
  },
  light: {
    background: "#e7ddc9",
    color: "#231c13",
    borderColor: "#e7ddc9",
    selectors: {
      "&:hover:not(:disabled), &[data-preview-state=hover]": {
        background: "#f1e9d9",
        borderColor: "#f1e9d9",
      },
      "&:active:not(:disabled), &[data-preview-state=pressed]": {
        background: "#cbbda4",
        borderColor: "#cbbda4",
      },
    },
  },
  text: {
    background: "transparent",
    borderColor: "transparent",
    color: vars.color.gold,
    textUnderlineOffset: "5px",
    selectors: {
      "&:hover:not(:disabled), &[data-preview-state=hover]": {
        background: "transparent",
        borderColor: "transparent",
        textDecoration: "underline",
      },
      "&:active:not(:disabled), &[data-preview-state=pressed]": {
        background: "transparent",
        borderColor: "transparent",
      },
    },
  },
  danger: {
    background: "rgba(92,40,36,.20)",
    color: vars.color.error,
    borderColor: "rgba(199,128,119,.50)",
  },
});
export const pill = style({ borderRadius: 999 });
export const iconOnly = style({
  padding: 10,
  width: 44,
  minHeight: 44,
  flexShrink: 0,
});
export const spinner = style({
  animation: `${turn} 900ms linear infinite`,
  "@media": {
    "(prefers-reduced-motion: reduce)": { animationDuration: "2400ms" },
  },
});
export const label = style({
  display: "block",
  color: vars.color.text,
  fontFamily: vars.font.utility,
  fontSize: 14,
  fontWeight: 500,
  marginBottom: 8,
});
export const helper = style({
  color: vars.color.muted,
  fontFamily: vars.font.utility,
  fontSize: 13,
  marginTop: 7,
  lineHeight: 1.5,
});
export const errorText = style({ color: vars.color.error });
export const fieldWrap = style({
  display: "flex",
  flexDirection: "column",
  minWidth: 0,
  width: "100%",
});
export const inputShell = style({
  display: "flex",
  alignItems: "center",
  minHeight: 48,
  gap: 10,
  padding: "0 14px",
  background: "rgba(11,15,22,.64)",
  border: `1px solid ${vars.color.line}`,
  borderRadius: vars.radius.control,
  color: vars.color.muted,
  transition: "border-color 140ms ease-out",
  selectors: {
    "&:hover:not([data-disabled=true])": { borderColor: vars.color.hoverLine },
    "&:focus-within": {
      borderColor: vars.color.goldMuted,
      boxShadow: "0 0 0 2px rgba(180,149,95,.12)",
    },
    "&[data-error=true]": { borderColor: "#b7756d" },
    "&[data-disabled=true]": { opacity: 0.4 },
  },
});
export const input = style({
  flex: 1,
  width: "100%",
  minWidth: 0,
  height: 46,
  padding: 0,
  background: "transparent",
  border: 0,
  outline: "none",
  fontFamily: vars.font.display,
  fontSize: 18,
  color: vars.color.text,
  selectors: {
    "&::placeholder": { color: vars.color.placeholder, opacity: 0.86 },
    "&::-webkit-search-cancel-button": { display: "none" },
  },
});
export const textarea = style([
  input,
  { height: 110, padding: "12px 0", resize: "vertical", lineHeight: 1.5 },
]);
export const selectTrigger = style([
  control,
  size.md,
  {
    width: "100%",
    justifyContent: "space-between",
    minWidth: 150,
    textAlign: "left",
    selectors: {
      "&[data-placeholder]": { color: vars.color.placeholder },
      "&[data-state=open]": { borderColor: vars.color.goldMuted },
    },
  },
]);
export const popup = style({
  background: "rgba(18,23,32,.98)",
  backdropFilter: "blur(20px)",
  border: `1px solid ${vars.color.line}`,
  borderRadius: 8,
  padding: 6,
  color: vars.color.text,
  boxShadow: "0 14px 36px rgba(0,0,0,.45)",
  zIndex: 80,
  minWidth: 220,
  maxHeight: "min(380px, var(--radix-select-content-available-height))",
  overflow: "auto",
  animation: `${enter} 140ms ease-out`,
  "@media": { "(prefers-reduced-motion: reduce)": { animation: "none" } },
});
export const selectPopup = style([
  popup,
  { minWidth: "var(--radix-select-trigger-width)" },
]);
export const item = style({
  display: "flex",
  alignItems: "center",
  gap: 10,
  minHeight: 40,
  padding: "9px 12px",
  borderRadius: 4,
  fontFamily: vars.font.display,
  fontSize: 17,
  outline: "none",
  cursor: "pointer",
  userSelect: "none",
  selectors: {
    "&[data-highlighted]": {
      color: "#fff0d1",
      background: "rgba(205,184,141,.12)",
    },
    "&[data-disabled]": { opacity: 0.35, pointerEvents: "none" },
    "&[data-state=checked]": { color: "#eed09e" },
  },
});
export const itemCheck = style({
  width: 18,
  flexShrink: 0,
  display: "inline-flex",
  color: vars.color.gold,
});
export const menuLabel = style({
  fontFamily: vars.font.utility,
  fontSize: 12,
  fontWeight: 500,
  color: vars.color.muted,
  padding: "9px 12px 5px",
});
export const separator = style({
  height: 1,
  background: "rgba(204,193,175,.17)",
  margin: "6px 5px",
});
export const overlay = style({
  position: "fixed",
  inset: 0,
  background: "rgba(3,6,10,.65)",
  backdropFilter: "blur(5px)",
  zIndex: 100,
});
export const dialog = style({
  position: "fixed",
  top: "50%",
  left: "50%",
  transform: "translate(-50%, -50%)",
  width: "min(480px, calc(100vw - 32px))",
  maxHeight: "calc(100dvh - 48px)",
  overflowY: "auto",
  padding: 28,
  background: "linear-gradient(140deg, #242a33, #141923)",
  border: `1px solid ${vars.color.line}`,
  borderRadius: 14,
  color: vars.color.text,
  zIndex: 101,
  boxShadow: "0 22px 70px rgba(0,0,0,.5)",
});
export const dialogTitle = style({
  margin: "0 36px 8px 0",
  fontFamily: vars.font.display,
  fontSize: 29,
  fontWeight: 500,
  lineHeight: 1.2,
});
export const dialogDescription = style({
  fontFamily: vars.font.utility,
  color: vars.color.muted,
  fontSize: 14,
  margin: "0 0 24px",
  lineHeight: 1.65,
});
export const close = style({ position: "absolute", right: 13, top: 13 });
export const dialogFooter = style({
  display: "flex",
  justifyContent: "flex-end",
  flexWrap: "wrap",
  gap: 12,
  marginTop: 28,
});
export const stack = style({
  display: "flex",
  flexDirection: "column",
  gap: 20,
});
export const choiceLabel = style({
  display: "inline-flex",
  alignItems: "center",
  gap: 10,
  color: vars.color.text,
  fontSize: 14,
  minHeight: 44,
  cursor: "pointer",
});
export const checkbox = style({
  padding: 0,
  width: 20,
  height: 20,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  background: "rgba(12,16,23,.7)",
  border: `1px solid ${vars.color.line}`,
  borderRadius: 4,
  color: "#16120d",
  cursor: "pointer",
  selectors: {
    "&[data-state=checked]": { background: "#d3b579", borderColor: "#d3b579" },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 3,
    },
    "&:disabled": { opacity: 0.4, cursor: "not-allowed" },
  },
});
export const switchRoot = style({
  width: 40,
  height: 24,
  borderRadius: 20,
  border: `1px solid ${vars.color.line}`,
  background: "#242b36",
  padding: 3,
  cursor: "pointer",
  selectors: {
    "&[data-state=checked]": { background: "#806a45" },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.gold}`,
      outlineOffset: 3,
    },
    "&:disabled": { opacity: 0.4 },
  },
});
export const switchThumb = style({
  display: "block",
  width: 16,
  height: 16,
  background: "#e6d7be",
  borderRadius: "50%",
  transition: "transform 160ms ease-out",
  selectors: { "&[data-state=checked]": { transform: "translateX(15px)" } },
});
export const radioGroup = style({ display: "flex", flexWrap: "wrap", gap: 20 });
export const radio = style([
  checkbox,
  {
    borderRadius: "50%",
    color: vars.color.gold,
    selectors: {
      "&[data-state=checked]": {
        background: "#11151e",
        borderColor: vars.color.goldMuted,
      },
    },
  },
]);
export const radioDot = style({
  width: 8,
  height: 8,
  background: vars.color.gold,
  borderRadius: "50%",
});
export const tabList = style({
  display: "inline-flex",
  flexWrap: "wrap",
  gap: 4,
  padding: 4,
  borderRadius: 7,
  background: "rgba(8,12,19,.65)",
  border: `1px solid rgba(205,196,176,.14)`,
});
export const tab = style({
  minHeight: 40,
  padding: "8px 18px",
  border: 0,
  borderRadius: 4,
  background: "transparent",
  color: vars.color.muted,
  fontFamily: vars.font.display,
  fontSize: 17,
  cursor: "pointer",
  selectors: {
    "&:hover": { color: vars.color.text, background: "rgba(215,198,163,.07)" },
    "&[data-state=active]": {
      background: "rgba(190,160,103,.15)",
      color: "#f0d7aa",
    },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 2,
    },
  },
});
export const accordion = style({ borderTop: `1px solid ${vars.color.line}` });
export const accordionItem = style({
  borderBottom: `1px solid ${vars.color.line}`,
});
export const accordionHeader = style({ margin: 0 });
export const accordionTrigger = style({
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  gap: 16,
  padding: "18px 4px",
  minHeight: 56,
  width: "100%",
  textAlign: "left",
  background: "transparent",
  border: 0,
  fontFamily: vars.font.display,
  color: vars.color.text,
  fontSize: 19,
  cursor: "pointer",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 2,
    },
  },
});
export const chevron = style({
  flexShrink: 0,
  transition: "transform 180ms ease-out",
  selectors: { '[data-state="open"] > &': { transform: "rotate(90deg)" } },
  "@media": { "(prefers-reduced-motion: reduce)": { transition: "none" } },
});
const open = keyframes({
  from: { height: 0, opacity: 0 },
  to: { height: "var(--radix-accordion-content-height)", opacity: 1 },
});
const closed = keyframes({
  from: { height: "var(--radix-accordion-content-height)", opacity: 1 },
  to: { height: 0, opacity: 0 },
});
export const accordionContent = style({
  overflow: "hidden",
  color: vars.color.muted,
  fontSize: 14,
  selectors: {
    "&[data-state=open]": { animation: `${open} 180ms ease-out` },
    "&[data-state=closed]": { animation: `${closed} 180ms ease-out` },
  },
  "@media": {
    "(prefers-reduced-motion: reduce)": { animation: "none !important" },
  },
});
export const accordionInner = style({ padding: "0 4px 22px" });
export const badge = style({
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  minHeight: 26,
  padding: "3px 9px",
  borderRadius: 4,
  fontSize: 12,
  fontFamily: vars.font.utility,
  color: "#dcccaf",
  background: "rgba(204,179,132,.10)",
});
export const badgeDanger = style({
  color: vars.color.error,
  background: "rgba(179,96,86,.16)",
});
export const alert = style({
  padding: 16,
  borderRadius: 6,
  background: "rgba(168,125,66,.12)",
  color: "#dac9aa",
  fontSize: 14,
});
export const stepper = style({
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
});
export const quantity = style({
  width: 48,
  minWidth: 48,
  height: 44,
  textAlign: "center",
  fontFamily: vars.font.utility,
  fontSize: 23,
  fontVariantNumeric: "tabular-nums",
  color: vars.color.text,
  border: 0,
  borderBottom: "1px solid transparent",
  background: "transparent",
  outline: "none",
  selectors: {
    "&:focus": { borderBottomColor: vars.color.goldMuted },
    "&::-webkit-inner-spin-button, &::-webkit-outer-spin-button": {
      WebkitAppearance: "none",
      margin: 0,
    },
  },
});
export const tooltip = style({
  padding: "7px 11px",
  background: "#242933",
  color: vars.color.text,
  fontFamily: vars.font.utility,
  fontSize: 12,
  borderRadius: 4,
  zIndex: 150,
  boxShadow: "0 6px 18px #0007",
});
export const sliderRoot = style({
  position: "relative",
  display: "flex",
  alignItems: "center",
  userSelect: "none",
  touchAction: "none",
  width: "100%",
  height: 44,
});
export const sliderTrack = style({
  position: "relative",
  background: "#353943",
  height: 3,
  flexGrow: 1,
  borderRadius: 2,
});
export const sliderRange = style({
  position: "absolute",
  background: vars.color.goldMuted,
  height: "100%",
  borderRadius: 2,
});
export const sliderThumb = style({
  display: "block",
  width: 16,
  height: 16,
  background: "#e2c693",
  borderRadius: "50%",
  boxShadow: "0 2px 6px #0007",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: 4,
    },
  },
});
globalStyle(`${control} svg`, {
  width: 19,
  height: 19,
  flexShrink: 0,
  strokeWidth: 1.6,
});
globalStyle(`${control} > span`, {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 9,
});
