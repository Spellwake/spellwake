import {
  createGlobalTheme,
  createThemeContract,
  globalStyle,
} from "@vanilla-extract/css";

export const vars = createThemeContract({
  color: {
    ground: null,
    surface: null,
    raised: null,
    text: null,
    muted: null,
    placeholder: null,
    line: null,
    hoverLine: null,
    gold: null,
    goldMuted: null,
    error: null,
  },
  font: { display: null, utility: null },
  space: { xs: null, sm: null, md: null, lg: null, xl: null },
  radius: { control: null, panel: null },
});
createGlobalTheme(":root", vars, {
  color: {
    ground: "#0c1017",
    surface: "rgba(12,16,23,.84)",
    raised: "#171c25",
    text: "#eee7db",
    muted: "#bdb5a7",
    placeholder: "#aaa397",
    line: "rgba(205,196,176,.30)",
    hoverLine: "rgba(224,204,164,.55)",
    gold: "#d7b77c",
    goldMuted: "#a98a59",
    error: "#e3a39a",
  },
  font: {
    display: '"Lora", Georgia, serif',
    utility: '"Albert Sans", system-ui, sans-serif',
  },
  space: { xs: "4px", sm: "8px", md: "16px", lg: "24px", xl: "48px" },
  radius: { control: "6px", panel: "12px" },
});
globalStyle("*", { boxSizing: "border-box" });
globalStyle("body", {
  margin: 0,
  background: vars.color.ground,
  color: vars.color.text,
  fontFamily: vars.font.utility,
  lineHeight: 1.5,
});
globalStyle("button,input,select,textarea", { font: "inherit" });
globalStyle("button", { WebkitTapHighlightColor: "transparent" });
globalStyle("::selection", { color: "#fff4dc", background: "#665134" });
globalStyle("input,textarea", { caretColor: vars.color.gold });
globalStyle("*", { scrollbarWidth: "thin", scrollbarColor: "#6c5d49 #11151d" });
