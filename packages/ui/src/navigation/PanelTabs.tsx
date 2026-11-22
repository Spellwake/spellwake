import {
  useId,
  useRef,
  useLayoutEffect,
  type CSSProperties,
  type ReactNode,
} from "react";
import * as Tabs from "@radix-ui/react-tabs";
import * as s from "./navigation.css";
export interface PanelTab {
  value: string;
  label: string;
  content: ReactNode;
}
export interface PanelTabsProps {
  label: string;
  tabs: readonly PanelTab[];
  value: string;
  onValueChange: (value: string) => void;
  /** "segmented" is a boxed set of equal choices with a filled current one, for use inside a panel. */
  /** "pill" draws separate rounded tabs with a rule after them, as the shared collection picker does. */
  treatment?: "underline" | "framed" | "segmented" | "pill";
  activationMode?: "automatic" | "manual";
}
/** `focusVisible` is in current browsers but not yet in TypeScript's DOM types. */
const PRESSED_FOCUS: FocusOptions & { focusVisible: boolean } = {
  focusVisible: false,
};
function revealTab(viewport: HTMLDivElement | null, tab: HTMLElement | null) {
  if (!viewport || !tab) return;
  const rect = tab.getBoundingClientRect(),
    bounds = viewport.getBoundingClientRect();
  if (rect.left < bounds.left) viewport.scrollLeft += rect.left - bounds.left;
  else if (rect.right > bounds.right)
    viewport.scrollLeft += rect.right - bounds.right;
}
/** Peer panels share a scrollable tab list, a single focus stop and a named, focusable panel. */
export function PanelTabs({
  label,
  tabs,
  value,
  onValueChange,
  treatment = "underline",
  activationMode = "automatic",
}: PanelTabsProps) {
  const anchor = "--sw-tab-" + useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const framed = treatment === "framed";
  const segmented = treatment === "segmented";
  const pill = treatment === "pill";
  // Segments and pills mark the chosen tab themselves, with no travelling line.
  const marked = !segmented && !pill;
  const viewport = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    revealTab(
      viewport.current,
      viewport.current?.querySelector<HTMLElement>('[aria-selected="true"]') ??
        null,
    );
  }, [value]);
  const mark = [s.underline, framed ? s.light : ""].join(" ");
  return (
    <Tabs.Root
      value={value}
      onValueChange={onValueChange}
      activationMode={activationMode}
      className={[s.theme, s.tabRoot].join(" ")}
      style={
        {
          "--sw-active-tab": anchor,
          "--sw-underline-inset": framed ? "15%" : "0px",
        } as CSSProperties
      }
    >
      <div ref={viewport} className={s.viewport}>
        <Tabs.List
          aria-label={label}
          className={[
            s.strip,
            framed ? s.framed : "",
            segmented ? s.segmented : "",
            pill ? s.pills : "",
          ].join(" ")}
        >
          {tabs.map((tab) => (
            <Tabs.Trigger
              key={tab.value}
              value={tab.value}
              className={[
                s.trigger,
                framed ? s.format : "",
                segmented ? s.segment : "",
                pill ? s.pill : "",
              ].join(" ")}
              onFocus={(event) =>
                revealTab(viewport.current, event.currentTarget)
              }
              // Radix focuses a pressed tab from script, which inherits the
              // focus ring of a text field that held focus. A pointer press
              // takes focus without one; the second focus call is then a no-op.
              onMouseDown={(event) => {
                if (event.button === 0 && !event.ctrlKey)
                  event.currentTarget.focus(PRESSED_FOCUS);
              }}
            >
              {tab.label}
              {value === tab.value && marked && (
                <span aria-hidden className={[mark, s.fallback].join(" ")} />
              )}
            </Tabs.Trigger>
          ))}
          {marked && (
            <span
              aria-hidden
              className={[mark, s.anchored].join(" ")}
              style={
                framed
                  ? ({
                      "--sw-underline-inset": "calc(anchor-size(width) * .15)",
                    } as CSSProperties)
                  : undefined
              }
            />
          )}
        </Tabs.List>
      </div>
      {tabs.map((tab) => (
        <Tabs.Content key={tab.value} value={tab.value} className={s.panel}>
          {tab.content}
        </Tabs.Content>
      ))}
    </Tabs.Root>
  );
}
