import { style } from "@vanilla-extract/css";
export const status = style({ position: "relative" });
export const srOnly = style({
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
export const shape = style({
  boxSizing: "border-box",
  display: "block",
  flex: "none",
  borderRadius: 8,
  background: "linear-gradient(108deg,#393a37,#323432 47%,#3e3c3a)",
  boxShadow: "inset 0 1px 2px #bdb1960a",
  selectors: {
    '&[data-tone="bright"]': {
      background: "linear-gradient(110deg,#757164,#67665e 68%,#706e63)",
    },
    '&[data-shape="portrait"]': {
      borderRadius: "50%",
      border: "2px solid #dbc59c",
      background: "linear-gradient(130deg,#282824,#191d1d)",
      boxShadow: "0 0 5px #080e12",
    },
    '&[data-shape="emblem"]': {
      borderRadius: 0,
      clipPath: "polygon(50% 0,100% 50%,50% 100%,0 50%)",
      background:
        "linear-gradient(90deg,transparent 45%,#c5b89c13 49%,#13151616 53%,transparent 57%), conic-gradient(from 0deg,#393733 0deg,#45413b 90deg,#37332e 90deg,#292926 180deg,#39352e 180deg,#49443b 270deg,#3f3d36 270deg,#514a41 360deg)",
    },
  },
});
