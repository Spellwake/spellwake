import { globalStyle, style } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";

export const dialog = style({
  vars: {
    "--confirm-width": "470px",
    "--confirm-pad": "36px",
    "--confirm-top": "66px",
    "--confirm-bottom": "37px",
    "--confirm-gap": "38px",
    "--confirm-title": "32px",
    "--confirm-copy": "23px",
    "--confirm-label": "23px",
    "--confirm-btn-height": "51px",
    "--confirm-radius": "20px",
    "--confirm-edge": "#bfb099",
    "--confirm-fill": "linear-gradient(115deg,#282420,#201c19)",
  },
  boxSizing: "border-box",
  width: "var(--confirm-width)",
  maxWidth: "calc(100% - 12px)",
  maxHeight: "calc(100% - 12px)",
  overflow: "auto",
  overscrollBehavior: "contain",
  border: "1.5px solid var(--confirm-edge)",
  borderRadius: "var(--confirm-radius)",
  padding: 0,
  margin: "auto",
  color: "#f4eee2",
  background: "var(--confirm-fill)",
  boxShadow: "0 4px 16px #0008,inset 0 0 0 1px #ffffff09",
  fontFamily: 'Baskerville,"Times New Roman",serif',
  textAlign: "center",
  textShadow: "0 1px 2px #000",
  WebkitFontSmoothing: "antialiased",
  selectors: {
    "&::backdrop": {
      background: "#090a0c80",
      overflow: "hidden",
      overscrollBehavior: "contain",
    },
    "&[data-appearance=social][data-compact=true]": {
      vars: {
        "--confirm-width": "420px",
        "--confirm-pad": "43px",
        "--confirm-top": "38px",
        "--confirm-bottom": "33px",
        "--confirm-gap": "37px",
        "--confirm-title": "35px",
        "--confirm-fill": "linear-gradient(125deg,#211e1c,#1b1918)",
      },
    },
    "&[data-appearance=market]": {
      textAlign: "start",
      vars: {
        "--confirm-width": "616px",
        "--confirm-pad": "30px",
        "--confirm-top": "15px",
        "--confirm-bottom": "23px",
        "--confirm-gap": "20px",
        "--confirm-title": "36px",
        "--confirm-label": "27px",
        "--confirm-btn-height": "56px",
        "--confirm-radius": "15px",
        "--confirm-edge": "#dcc7a0",
        "--confirm-fill": "linear-gradient(120deg,#181818,#0e1012 70%)",
      },
    },
    "&[data-appearance=lobby]": {
      vars: {
        "--confirm-width": "513px",
        "--confirm-pad": "45px",
        "--confirm-top": "38px",
        "--confirm-bottom": "32px",
        "--confirm-gap": "30px",
        "--confirm-title": "40px",
        "--confirm-label": "25px",
        "--confirm-btn-height": "49px",
        "--confirm-edge": "#dfc9a1",
        "--confirm-fill": "linear-gradient(125deg,#11171a,#0a0e10)",
      },
    },
    "&[data-appearance=lobby][data-compact=true]": {
      vars: {
        "--confirm-width": "504px",
        "--confirm-pad": "56px",
        "--confirm-top": "24px",
        "--confirm-bottom": "24px",
        "--confirm-title": "35px",
        "--confirm-gap": "29px",
        "--confirm-btn-height": "46px",
      },
    },
    "&[data-appearance=trade]": {
      vars: {
        "--confirm-width": "572px",
        "--confirm-pad": "37px",
        "--confirm-top": "29px",
        "--confirm-bottom": "31px",
        "--confirm-gap": "29px",
        "--confirm-title": "42px",
        "--confirm-label": "26px",
        "--confirm-edge": "#e5d4b8",
        "--confirm-fill": "linear-gradient(130deg,#101212,#080a0b)",
      },
    },
    "&[data-appearance=match]": {
      textAlign: "start",
      vars: {
        "--confirm-width": "524px",
        "--confirm-pad": "30px",
        "--confirm-top": "20px",
        "--confirm-bottom": "26px",
        "--confirm-gap": "29px",
        "--confirm-title": "35px",
        "--confirm-copy": "22px",
        "--confirm-label": "26px",
        "--confirm-edge": "#ebd4a7",
        "--confirm-fill": "linear-gradient(120deg,#272823,#222321)",
        "--confirm-btn-height": "54px",
      },
    },
  },
});
export const body = style({
  position: "relative",
  padding:
    "var(--confirm-top) min(var(--confirm-pad),13vw) var(--confirm-bottom)",
});
export const title = style({
  display: "block",
  textAlign: "inherit",
  margin: 0,
  color: "inherit",
  fontFamily: "inherit",
  fontSize: "var(--confirm-title)",
  fontWeight: 400,
  lineHeight: 1.2,
  overflowWrap: "anywhere",
});
export const description = style({
  margin: "14px 0 0",
  fontSize: "var(--confirm-copy)",
  lineHeight: 1.25,
  color: "inherit",
  overflowWrap: "anywhere",
});
globalStyle(`${dialog}[data-divider=true] ${title}::after`, {
  content: '""',
  display: "block",
  height: 1,
  marginTop: 14,
  background:
    "linear-gradient(90deg,transparent,#74675865 12%,#74675865 88%,transparent)",
});
globalStyle(`${dialog}[data-appearance=match] :is(h2,p)`, {
  fontFamily: '"Albert Sans",system-ui,sans-serif',
});
globalStyle(`${dialog}[data-appearance=match] ${description}`, {
  marginTop: 18,
});
export const actions = style({
  display: "flex",
  justifyContent: "center",
  flexWrap: "wrap",
  gap: "8px 18px",
  marginTop: "var(--confirm-gap,24px)",
});
globalStyle(`${actions} button`, {
  flex: "1 1 120px",
  minWidth: 0,
  height: "auto",
  minHeight: "var(--confirm-btn-height,48px)",
  "@media": {
    "(pointer: coarse)": {
      minHeight: "max(var(--confirm-btn-height,48px), 44PX)",
    },
  },
  padding: "6px 10px",
  font: '400 var(--confirm-label,22px)/1.15 Baskerville,"Times New Roman",serif',
  borderRadius: 12,
  color: "#f5eee1",
  border: "1px solid #7a6f62",
  boxShadow: "inset 0 0 0 1px #f3dfc30c",
  background: "linear-gradient(135deg,#292928,#212120)",
});
globalStyle(`${actions} button:last-child`, {
  borderColor: "#e3c276",
  color: "#f7e9c4",
});
globalStyle(`${actions}[data-tone=danger] button:last-child`, {
  borderColor: "#a64c42",
  color: "#fff0df",
  background: "linear-gradient(120deg,#4b201e,#401d1b)",
});
globalStyle(`${dialog}[data-appearance=match] ${actions} button:last-child`, {
  background: "linear-gradient(110deg,#813831,#713029)",
  borderColor: "#d16857",
});
globalStyle(
  `${dialog}:is([data-appearance=lobby],[data-appearance=trade]) ${actions} button:not(:last-child), ${dialog}[data-appearance=lobby] ${actions} button:last-child`,
  {
    background: "linear-gradient(120deg,#1b1c1c,#111416)",
    borderColor: "#bbac95",
  },
);
globalStyle(`${dialog}[data-appearance=lobby] ${actions} button:last-child`, {
  borderColor: "#dfc591",
});
globalStyle(`${actions} button:enabled:hover`, { filter: "brightness(1.15)" });
export const close = style({
  position: "absolute",
  right: 16,
  top: 14,
  display: "grid",
  placeItems: "center",
  width: 32,
  height: 32,
  borderRadius: 6,
  padding: 0,
  border: 0,
  color: "#f8f0dd",
  background: "transparent",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": { outline: "2px solid #ffe2a1" },
    "&:enabled:hover": { background: "#ffffff15" },
    "&:disabled": { opacity: 0.5, cursor: "default" },
  },
  "@media": {
    [standardMotion]: { transition: `background-color ${feedbackTransition}` },
  },
});
globalStyle(`${close} svg`, { width: 25, height: 25 });

globalStyle(
  `${dialog}[data-appearance=trade] ${actions}[data-tone=danger] button:last-child`,
  {
    background: "linear-gradient(115deg,#471a17,#380f10 70%,#421415)",
    borderColor: "#ac3a31",
  },
);
globalStyle(`${actions} button:focus-visible`, {
  outline: "2px solid #ffe6ac",
  outlineOffset: 2,
});

const wideTrade = `${dialog}[data-appearance=trade][data-wide]`;
globalStyle(wideTrade, {
  width: 704,
  minHeight: "min(367px,100dvh)",
  maxWidth: "100%",
  maxHeight: "100%",
  textAlign: "start",
  vars: {
    "--confirm-pad": "45px",
    "--confirm-top": "38px",
    "--confirm-bottom": "77px",
    "--confirm-gap": "32px",
    "--confirm-title": "48px",
    "--confirm-copy": "28px",
    "--confirm-label": "32px",
    "--confirm-btn-height": "68px",
    "--confirm-fill": "linear-gradient(125deg,#15181b,#0e1114 60%,#191c1d)",
  },
});
globalStyle(`${wideTrade} ${title}`, { paddingRight: 55 });
globalStyle(`${wideTrade} ${description}`, {
  fontFamily: '"Times New Roman",serif',
  letterSpacing: "-.025em",
  marginInlineEnd: -13,
});
globalStyle(`${wideTrade} ${actions}`, {
  position: "relative",
  paddingTop: 38,
  gap: "12px 24px",
});
globalStyle(`${wideTrade} ${actions}::before`, {
  content: '""',
  position: "absolute",
  inset: "0 -12px auto",
  height: 1,
  background: "#9d9482",
});
globalStyle(`${wideTrade} ${close}`, {
  width: 52,
  height: 50,
  right: 29,
  top: 35,
  border: "1px solid #bba88a",
  background: "linear-gradient(#242525,#101418)",
});
globalStyle(`${wideTrade} ${actions}[data-tone=gold] button:last-child`, {
  background: "linear-gradient(130deg,#453e33,#24231f)",
  color: "#f5eddf",
});

globalStyle(`${wideTrade} ${actions} button:focus-visible`, {
  outlineOffset: -2,
});
// Close grows to a fingertip on touch screens; its icon stays the same size.
globalStyle(`${close}${close}`, {
  "@media": {
    "(pointer: coarse)": { width: "44PX", height: "44PX", top: 8, right: 10 },
  },
});
