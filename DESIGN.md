---
name: Spellwake
description: Approved hybrid controls over an illustrated world
colors:
  ground: "#0c1017"
  surface: "rgba(12,16,23,.84)"
  raised: "#171c25"
  controlGlass: "rgba(14,17,23,.58)"
  primaryGlass: "rgba(65,48,29,.48)"
  text: "#eee7db"
  muted: "#bdb5a7"
  placeholder: "#aaa397"
  line: "rgba(205,196,176,.30)"
  hoverLine: "rgba(224,204,164,.55)"
  gold: "#d7b77c"
  goldMuted: "#a98a59"
  error: "#e3a39a"
  inboxGlass: "rgba(30,29,29,.97)"
  inboxLine: "rgba(205,184,144,.46)"
  inboxCard: "rgba(215,205,190,.055)"
  inboxAccent: "#f7c679"
  inboxOutgoing: "#645037"
  inboxActionLine: "#ffb62d"
  inboxOnline: "#70df9a"
  inboxIncomingStart: "#272626"
  inboxIncomingEnd: "#2d2d2e"
  inboxOutgoingEnd: "#685439"
  inboxCardShade: "rgba(0,10,20,.3)"
  inboxPrimaryGlass: "rgba(22,22,22,.3)"
typography:
  display:
    fontFamily: '"Lora", Georgia, serif'
    fontSize: "clamp(30px,3.2vw,44px)"
    fontWeight: 500
    lineHeight: 1.15
    letterSpacing: "-.018em"
  control:
    fontFamily: '"Lora", Georgia, serif'
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.15
  utility:
    fontFamily: '"Albert Sans", system-ui, sans-serif'
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.5
  conversation:
    fontFamily: '"SpellwakeConversation", Georgia, serif'
    fontSize: "23.5px"
    fontWeight: 300
    lineHeight: 1.15
    fontVariation: '"wght" 300, "opsz" 20'
  conversation-body:
    fontFamily: '"SpellwakeConversation", Georgia, serif'
    fontSize: "17.8px"
    fontWeight: 350
    lineHeight: "24px"
    fontVariation: '"wght" 350, "opsz" 20'
rounded:
  control: "6px"
  panel: "12px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "48px"
components:
  button-primary:
    backgroundColor: "{colors.primaryGlass}"
    textColor: "#f7e2bb"
    rounded: "{rounded.control}"
    padding: "11px 20px"
  button-secondary:
    backgroundColor: "{colors.controlGlass}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "11px 20px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text}"
  button-transparent:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
  button-light:
    backgroundColor: "#e7ddc9"
    textColor: "#231c13"
  button-text:
    backgroundColor: "transparent"
    textColor: "{colors.gold}"
  button-danger:
    backgroundColor: "rgba(92,40,36,.20)"
    textColor: "{colors.error}"
  input:
    backgroundColor: "{colors.controlGlass}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 14px"
---

# Design System: Spellwake

The component specifications below document the archived implementation and serve as the starting point for library reviews.

## Overview

**Creative North Star: "Approved Hybrid Atelier / Vault"**

The approved hybrid joins Moonlit Atelier main-content controls with Astral Vault top and side navigation. Dark illustrated surroundings supply atmosphere; quiet, readable control surfaces sit above them. This describes the implemented component workshop, not a reconstruction of every object in the reference composition.

The user selected `assets/reference/hybrid-ui/reference.png` on October 31, 2026 and authorized replacing the earlier control appearance. That decision supersedes older palette and low-fidelity constraints in PRODUCT.md. The reference establishes visual character, but its generated lettering cannot establish an exact font identity.

The people-rail inbox has a scoped reference variant selected on November 2, 2026: `assets/ui-study/social-inbox-directions-2026-11-02/04-thread-with-people-rail-v4-invitations.png`. Its warmer conversation materials and serif roles apply to that surface. The surrounding application retains the hybrid controls above.

**Key Characteristics:**

- Smoked dark surfaces and restrained brass edges.
- Serif navigation and controls paired with compact utility text.
- Gold line-and-diamond selection distinct from hover.
- Illustrated surroundings remain owned by the consuming app.

## Colors

Near-black glass and warm ivory establish readability; muted brass carries emphasis.

### Primary

- **Warm Brass** (`gold`): active accents and text actions.
- **Quiet Brass** (`goldMuted`): primary outlines, focus and selected edges.

### Neutral

- **Midnight Ground** (`ground`): the app fallback behind illustrated surroundings.
- **Smoked Glass** (`surface`) and **Raised Slate** (`raised`): dark surface roles.
- **Warm Ivory** (`text`), **Ash Linen** (`muted`) and **Stone Placeholder** (`placeholder`): main, supporting and empty-field text.
- **Quiet Rule** (`line`) and **Warm Hover Rule** (`hoverLine`): edges at rest and hover.
- **Muted Rose** (`error`): errors and destructive action text; never the default accent.

The inbox variant uses `inboxGlass` for its warmer panel, `inboxLine` for its outer edge and portrait rules, `inboxCard` for quiet invitation/composer fills, `inboxOutgoing` for outgoing bubbles, and `inboxAccent` / `inboxActionLine` for reference-specific action and selection emphasis. The remaining inbox roles define the online dot, incoming/outgoing gradient endpoints, invitation shading and primary action fill. These roles do not replace the general control palette.

**The Environment Boundary Rule.** Atmosphere belongs to the app; shared controls must remain readable without a backdrop image.

## Typography

**Display Font:** Lora, with Georgia and serif fallback.
**Body Font:** Albert Sans, with system-ui and sans-serif fallback.
**Label/Mono Font:** Albert Sans for labels, help and status. Workshop code examples use the system monospace stack.

The user committed to Lora on October 31, 2026 for headings, navigation, fields and action labels. Libre Baskerville remains a workshop comparison candidate. The inbox variant uses Source Serif 4 through the dedicated `SpellwakeConversation` font-face alias for headings, messages, timestamps, actions and composer text. Its `conversation` and `conversationBody` theme roles isolate that choice from Lora and Albert Sans elsewhere. Both hosts self-host the Source Serif 4 WOFF2 and OFL license; the workshop also retains its original comparison family declaration. Font files and loading declarations belong to the consuming apps. The UI package exports family tokens and ships no font files or font-loading declarations.

### Hierarchy

- **Display:** the frontmatter display role is the workshop h1 treatment.
- **Headline:** serif (500, 29px, 1.2) for section and dialog titles.
- **Title:** serif (500, 22px, 1.3) for workshop subheadings.
- **Control:** the frontmatter control role; small and large buttons adjust to 16px and 20px.
- **Body:** utility text (14px, 1.7 in workshop paragraphs), constrained to 72ch in the workshop.
- **Label:** utility text (500, 14px); helper text is 13px, group labels 12px.

**The Chosen Serif Rule.** Lora remains the approved general UI serif. The scoped inbox uses Source Serif 4 to match its approved reference; neither choice identifies the generated raster lettering.

## Layout

The frontmatter spacing scale records the actual shared theme. Intermediate values in component styles are local geometry, not additional global tokens. Controls have minimum heights of 38px, 48px and 56px; narrow-screen shared action controls rise to at least 44px below 640px. Icon actions are 44px square. Inputs and selects use a 48px shell.

Desktop top navigation is at least 80px tall and the side rail is 192px wide. At 900px and below, top destinations move to their own horizontal row and the side rail becomes a horizontal strip. The workshop uses a 1500px maximum canvas with fluid gutters; its sample toolbar collapses at 1050px and 500px. Its multicolumn examples collapse at 900px or 800px. These workshop layouts demonstrate reuse, not a mandatory page grid.

## Elevation & Depth

Controls are shallow and mostly flat; thin warm rules and tonal changes establish hierarchy. Navigation uses smoked gradients and backdrop blur. Popups and dialogs receive structural dark shadows to separate floating interaction from the illustrated environment. Reduced-motion rules suppress popup and accordion entrances and most control transitions; the busy spinner slows rather than disappears.

### Shadow Vocabulary

- **Popup:** `0 14px 36px rgba(0,0,0,.45)` for menus and selects.
- **Dialog:** `0 22px 70px rgba(0,0,0,.5)` for focused tasks.
- **Field focus:** `0 0 0 2px rgba(180,149,95,.12)` with a brass border.

**The Selection Rule.** Hover must remain weaker than selection; navigation selection keeps its own line and diamond.

## Shapes

Shared field and action corners use the control token. The panel token is available for larger surfaces; existing popup and modal geometry uses local 8px and 14px corners. Collection Play uses the 6px control radius rather than a pill, following Moonlit Atelier. Button has no alternate pill shape; all action variants use the same control radius. Navigation remains rectilinear; its small gold diamond is a selection marker. Do not make every control a decorative fantasy artifact.

## Components

### Buttons

Restrained, readable actions share anatomy and size variants, with optional leading/trailing icons and a loading spinner.

| Variant     | Assignment and state behavior                                                                                           |
| ----------- | ----------------------------------------------------------------------------------------------------------------------- |
| Primary     | Warm dark tint and brass outline over a blurred, saturated backdrop; stronger brass on hover, darker on press.          |
| Secondary   | Quieter dark glass with subdued trim and the same blurred, saturated backdrop. Collection Play uses the control radius. |
| Ghost       | Clear at rest; faint warm fill on hover and press.                                                                      |
| Transparent | Fully clear at rest, hover and press; only text/icon color changes. Use for field clear and dialog close.               |
| Light       | Ivory fill with dark ink; lighter hover, darker press.                                                                  |
| Text        | Clear, gold text with an underline on hover.                                                                            |
| Danger      | Muted rose text and low-intensity destructive tint.                                                                     |

Focus uses an offset gold outline. Loading disables the action and exposes `aria-busy`; disabled controls reduce opacity. Selection is opt-in and independent of hover. Workshop preview-state attributes demonstrate visuals; production consumers should rely on native interaction states.

Primary and secondary buttons, input shells and select triggers share `effect.controlBackdrop`: blur 12px and saturation 135%, including the WebKit-prefixed property. Fields, selectors and secondary buttons use `controlGlass`; primary uses the warmer `primaryGlass` with its brass edge. Translucent fills remain the fallback. Fully transparent and ghost controls do not inherit this effect.

### Inputs / Fields

TextInput, TextArea and SearchInput use quiet dark shells, serif entered text and visible utility labels. Focus adds a muted brass border and soft ring; errors supply an associated message and error edge. Search clear stays fully transparent. A label is required even when visually hidden by the consumer.

### Menus and Selects

Grouped labels and dividers organize the popup rather than adding competing cards. Radix supplies keyboard traversal, disabled item behavior, selection and focus mechanics. Select shows the checked item distinctly from the highlighted candidate. Menu entries model labels, dividers and actions explicitly.

### Dialogs

Modal provides title, description, X close, outside-click and Escape dismissal. Focus enters the first field where available, stays managed by Radix and returns to the invoking element. FieldStack and DialogFooter support compact forms; validation and persistence remain the consumer's responsibility.

### Navigation

TopNavigation and SideNavigation use the approved gold symbol, smoked surfaces and quiet hover. The current destination has warm text plus a gold line and diamond. The shared `effect.navLineGlow` and `effect.navDiamondGlow` add a restrained warm bloom, brightest at the diamond, with a faint directional wash into the selected item. This is static selection feedback, not a pulsing animation. Account and notification actions use the same shared control family. Short fading dividers separate logo from destinations, coins from notifications, and notifications from account. On desktop, a small curved border beneath the logo interrupts the header baseline; it disappears when destinations wrap to their own row. Narrow layouts preserve destination access through horizontal rows.

### Choices, Feedback and Quantity

Checkbox, Switch, RadioOptions, TabGroup and Slider provide controlled selection; Accordion provides disclosure. Badge, Alert, Tooltip and Spinner carry compact feedback. Quantity reserves numeric width, supports direct integer editing, commits on Enter or blur, cancels on Escape, disables increment at the maximum and turns decrement at one copy into explicit removal.

Cards in the small library composition are workshop examples, not a shared deck-card or deck-box API. Existing deck-box and candle effects remain separate integration work.

### People-rail Inbox

A warm conversation panel pairs one active thread with a vertical portrait rail. The panel is at most 560px wide and 579px high, further constrained by Radix available height and the viewport. The desktop rail is 81px, reducing to 64px below 480px. Below 640px, the panel width is the viewport minus 32px. History scrolls independently above the compact composer; narrow layouts wrap timestamps and invitation metadata.

Trade invitations contain an exchange icon, title, Trade and Decline. Game invitations contain a group icon, title, format and arrangement, Join and Decline. These cards do not preview items, occupancy or decks. Inbox action controls retain shared Button behavior with scoped reference typography, radii and trim. Portraits remain host supplied; the local demo's three portraits and request emblem are in-progress crops from the approved reference atlas, not production asset approval. The optional host-owned `requestBadgeFraming` is presentation data; connected people keep the existing vector fallback. Panel, text and action controls remain real DOM.

## Do's and Don'ts

### Do:

- **Do** use the shared controls and vanilla-extract theme for new application work.
- **Do** keep visible gold keyboard focus, associated labels and helper/error text.
- **Do** preserve the actual gold logo and integrate approved deck-box and candle effects separately.
- **Do** identify workshop data and outcomes as illustrative.

### Don't:

- **Don't** import old study CSS into the new UI package.
- **Don't** turn fully transparent clear or close actions into filled hover buttons.
- **Don't** use white focus outlines, bright orange or green filled controls in this world.
- **Don't** treat the workshop as production auth, economy, persistence or match behavior.
