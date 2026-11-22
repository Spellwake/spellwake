import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactElement,
} from "react";
import {
  ActionButton,
  Dialog,
  FeedbackMessages,
  RangeSlider,
  TextField,
} from "@spellwake/ui";
import type {
  AccountSettingsSource,
  ArtChoice,
} from "../../account/account-sources";
import { artPlacement } from "../../account/Portrait";
import * as portrait from "../../account/portrait.css";
import {
  CENTRED,
  MAX_ZOOM,
  MIN_ZOOM,
  pannedCrop,
  zoomedCrop,
  type PortraitCrop,
} from "../../account/portrait-crop";
import { useCommand, useNotice } from "./operation";
import * as s from "./account.css";

type Picture = NonNullable<
  Extract<AccountSettingsSource, { status: "ready" }>["picture"]
>;
/** How far one arrow key moves the picture, as a share of the circle. */
const KEY_STEP = 0.04;
const USUAL_ASPECT = 626 / 457;

/**
 * The crop stage: the artwork under a fixed circle. Drag, or use the arrow
 * keys, to move it. The shading and guide lines are drawn over the picture and
 * are never part of what is saved.
 */
function CropStage({
  art,
  crop,
  aspect,
  disabled,
  onCrop,
  onAspect,
}: {
  art: ArtChoice;
  crop: PortraitCrop;
  aspect: number;
  disabled: boolean;
  onCrop: (next: (crop: PortraitCrop) => PortraitCrop) => void;
  onAspect: (aspect: number) => void;
}): ReactElement {
  const stage = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointer: number; x: number; y: number } | null>(null);
  // Choosing artwork moves on to this stage, so focus follows to it.
  useEffect(() => stage.current?.focus({ preventScroll: true }), []);
  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    if (disabled || !event.isPrimary || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointer: event.pointerId,
      x: event.clientX,
      y: event.clientY,
    };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    const from = drag.current;
    if (!from || from.pointer !== event.pointerId) return;
    const size = event.currentTarget.clientWidth;
    if (size === 0) return;
    const dx = (event.clientX - from.x) / size;
    const dy = (event.clientY - from.y) / size;
    drag.current = { ...from, x: event.clientX, y: event.clientY };
    onCrop((current) => pannedCrop(current, dx, dy, aspect));
  };
  const endDrag = (): void => {
    drag.current = null;
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (disabled) return;
    const step: Record<string, readonly [number, number]> = {
      ArrowLeft: [-KEY_STEP, 0],
      ArrowRight: [KEY_STEP, 0],
      ArrowUp: [0, -KEY_STEP],
      ArrowDown: [0, KEY_STEP],
    };
    const move = step[event.key];
    if (!move) return;
    event.preventDefault();
    onCrop((current) => pannedCrop(current, move[0], move[1], aspect));
  };
  return (
    <div
      ref={stage}
      className={s.cropStage}
      role="group"
      aria-label="Picture position. Drag, or use the arrow keys, to move it."
      // In a drawer, dragging here moves the picture, never the drawer.
      data-vaul-no-drag=""
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={onKeyDown}
    >
      <img
        className={portrait.art}
        src={art.artUrl}
        alt=""
        draggable={false}
        data-art-ready="true"
        style={artPlacement(crop, aspect)}
        onLoad={(event) => {
          const image = event.currentTarget;
          if (image.naturalWidth > 0 && image.naturalHeight > 0)
            onAspect(image.naturalWidth / image.naturalHeight);
        }}
      />
      <span className={s.cropShade} />
      <span className={s.cropGuide} />
    </div>
  );
}

/**
 * Chooses a picture in two stages inside one fixed shell: search any card's
 * artwork by name, then place it under the circle. Save commits only the
 * picture. Closing at any point discards the choice.
 */
export function PictureEditor({
  picture,
  onClose,
}: {
  picture: Picture;
  onClose: () => void;
}): ReactElement {
  const { search } = picture;
  const [art, setArt] = useState<ArtChoice | null>(null);
  const [crop, setCrop] = useState<PortraitCrop>(CENTRED);
  const [aspect, setAspect] = useState(USUAL_ASPECT);
  const command = useCommand();
  const notice = useNotice();
  const { say } = notice;
  const hasFailed = search.state === "failed";
  useEffect(() => {
    if (hasFailed) say("Couldn’t load artwork.");
  }, [hasFailed, say]);
  // The search belongs to this visit: it starts empty and is left empty.
  const { setQuery } = search;
  useEffect(() => () => setQuery(""), [setQuery]);

  const save = (): void => {
    if (!art) return;
    say("Saving picture…", "working");
    void command
      .run(() => picture.save(art, crop))
      .then((outcome) => {
        if (outcome?.status === "success") onClose();
        else if (outcome) say(outcome.message);
      });
  };
  const zoomPercent = Math.round(crop.scale * 100);
  return (
    <Dialog
      open
      title="Profile picture"
      closeSide="start"
      className={s.pictureShell}
      busy={command.isBusy}
      onClose={onClose}
      {...(art
        ? {
            actions: (
              <>
                <ActionButton
                  className={s.action}
                  disabled={command.isBusy}
                  onClick={onClose}
                >
                  Cancel
                </ActionButton>
                <ActionButton
                  className={`${s.action} ${s.goldAction}`}
                  pending={command.isBusy}
                  onClick={save}
                >
                  Save
                </ActionButton>
              </>
            ),
          }
        : {})}
    >
      <div
        className={s.pictureBody}
        data-picture-stage={art ? "crop" : "choose"}
      >
        {art ? (
          <>
            <p className={s.cropHint}>Drag to reposition.</p>
            <CropStage
              art={art}
              crop={crop}
              aspect={aspect}
              disabled={command.isBusy}
              onCrop={setCrop}
              onAspect={(next) => {
                setAspect(next);
                setCrop((current) => zoomedCrop(current, current.scale, next));
              }}
            />
            <p className={s.cropCredit}>
              {art.artist ? `${art.name} · ${art.artist}` : art.name}
            </p>
            <div className={s.zoom} data-vaul-no-drag="">
              <RangeSlider
                label="Zoom"
                min={MIN_ZOOM * 100}
                max={MAX_ZOOM * 100}
                step={5}
                value={zoomPercent}
                valueText={`${zoomPercent}%`}
                showStepButtons
                disabled={command.isBusy}
                onValueChange={(value) =>
                  setCrop((current) => zoomedCrop(current, value / 100, aspect))
                }
              />
            </div>
          </>
        ) : (
          <>
            <TextField
              label="Search artwork by card name"
              hideLabel
              type="search"
              data-autofocus=""
              autoComplete="off"
              spellCheck={false}
              enterKeyHint="search"
              value={search.query}
              onValueChange={search.setQuery}
            />
            {search.state === "ready" && search.results.length === 0 ? (
              <div className={s.noArt} role="status">
                <p className={s.noArtTitle}>No artwork found.</p>
                <p className={s.text}>Try another search.</p>
              </div>
            ) : (
              <div className={s.artGrid} aria-busy={search.state === "loading"}>
                {search.results.map((choice) => (
                  <button
                    key={choice.id}
                    type="button"
                    className={s.artTile}
                    onClick={() => {
                      setCrop(CENTRED);
                      setAspect(USUAL_ASPECT);
                      setArt(choice);
                    }}
                  >
                    <span className={s.artFrame}>
                      <img src={choice.artUrl} alt="" loading="lazy" />
                    </span>
                    {choice.name}
                  </button>
                ))}
              </div>
            )}
          </>
        )}
        <div className={s.childNotice}>
          <FeedbackMessages
            label="Picture messages"
            presentation="toast"
            items={notice.items}
          />
        </div>
      </div>
    </Dialog>
  );
}
