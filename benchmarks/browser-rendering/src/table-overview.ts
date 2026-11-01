/** Viewer-local overview navigation. Seat positions come from the actual table, never a second layout. */
export type OverviewSeat = { x: number; z: number; name: string };
export function overviewDots(seats: readonly OverviewSeat[], viewer: number) {
  const xs = seats.map((seat) => seat.x),
    zs = seats.map((seat) => seat.z);
  const minX = Math.min(...xs),
    maxX = Math.max(...xs),
    minZ = Math.min(...zs),
    maxZ = Math.max(...zs);
  return seats.map((seat, index) => ({
    seat: index + 1,
    x: 24 + (maxX === minX ? 0 : ((seat.x - minX) / (maxX - minX) - 0.5) * 26),
    y: 24 + (maxZ === minZ ? 0 : ((seat.z - minZ) / (maxZ - minZ) - 0.5) * 22),
    viewer: index + 1 === viewer,
  }));
}
export function createTableOverview(
  host: HTMLElement,
  choose: (seat: number) => void,
  names: readonly string[] = [],
  portraits: readonly (HTMLImageElement | HTMLCanvasElement | null)[] = [],
) {
  const doc = host.ownerDocument,
    lifetime = new AbortController();
  const root = doc.createElement("div");
  root.className = "table-overview";
  const style = doc.createElement("style");
  // Tauri authorizes bundled styles with a nonce; runtime styles need the same authorization.
  const nonce = doc.head.querySelector<HTMLStyleElement>("style[nonce]")?.nonce;
  if (nonce) style.nonce = nonce;
  style.textContent = `
.table-overview{position:absolute;inset:0;pointer-events:none;z-index:1;font-family:"Albert Sans",sans-serif;--overview-size:48px;--overview-inset:16px}
.table-overview button{pointer-events:auto;display:grid;place-items:center;padding:0;border:1px solid var(--line);border-radius:50%;background:var(--panel);color:var(--ink);cursor:pointer}
.table-overview button:hover{border-color:var(--accent);background:var(--ground)}
.table-overview button:focus-visible{outline:2px solid var(--accent);outline-offset:4px}
.table-overview button:disabled{opacity:.45;cursor:default}
.table-overview-toggle{position:absolute;top:var(--overview-inset);left:var(--overview-inset);width:var(--overview-size);height:var(--overview-size)}
.table-overview-toggle svg{display:block}
.table-overview-toggle[aria-pressed=true] .overview-layout,.table-overview-toggle[aria-pressed=false] .overview-home{display:none}
.table-overview-seat{position:absolute;width:44px;height:44px;transform:translate(-50%,-50%)}
.table-overview-seat canvas{display:block;width:100%;height:100%;border-radius:inherit}
.table-overview-seat span{position:absolute;top:calc(100% + 6px);font:500 13px/1.3 "Albert Sans",sans-serif;white-space:nowrap;color:var(--ink)}
.table-overview-seat[data-viewer=true]{border-color:var(--accent)}
.table-overview [hidden]{display:none!important}`;
  const toggle = doc.createElement("button");
  toggle.type = "button";
  toggle.className = "table-overview-toggle";
  toggle.title = "Table overview";
  toggle.setAttribute("aria-label", "Table overview");
  toggle.setAttribute("aria-pressed", "false");
  let overview = false;
  toggle.addEventListener("click", () => choose(overview ? viewerSeat : 0), {
    signal: lifetime.signal,
  });
  root.append(style, toggle);
  host.append(root);
  let signature = "",
    viewerSeat = 1;
  const controls: HTMLButtonElement[] = [];
  return {
    update(
      seats: readonly OverviewSeat[],
      viewer: number,
      active: boolean,
      visible: boolean,
      blocked: boolean,
      project: (seat: OverviewSeat) => { x: number; y: number },
    ): void {
      overview = active;
      viewerSeat = viewer;
      root.hidden = !visible;
      toggle.disabled = blocked;
      toggle.title = active ? "Return to your board" : "Table overview";
      toggle.setAttribute("aria-label", toggle.title);
      toggle.setAttribute("aria-pressed", String(active));
      const key = JSON.stringify([seats, viewer]);
      if (signature !== key) {
        signature = key;
        toggle.innerHTML = `<svg viewBox="0 0 48 48" width="36" height="36" aria-hidden="true"><g class="overview-layout">${overviewDots(
          seats,
          viewer,
        )
          .map(
            (dot) =>
              `<circle cx="${dot.x}" cy="${dot.y}" r="${dot.viewer ? 3.8 : 3.2}" fill="${dot.viewer ? "var(--accent)" : "currentColor"}" fill-opacity="${dot.viewer ? 1 : 0.6}"/>`,
          )
          .join(
            "",
          )}</g><circle class="overview-home" cx="24" cy="24" r="5" fill="var(--accent)"/></svg>`;
        for (const control of controls) control.remove();
        controls.length = 0;
        seats.forEach((_seat, index) => {
          const control = doc.createElement("button");
          control.type = "button";
          control.className = "table-overview-seat";
          control.dataset.viewer = String(index + 1 === viewer);
          const portrait = portraits[index];
          if (portrait) {
            const image = doc.createElement("canvas");
            image.width = image.height = 128;
            image.setAttribute("aria-hidden", "true");
            const context = image.getContext("2d");
            const size = Math.min(portrait.width, portrait.height);
            context?.drawImage(
              portrait,
              (portrait.width - size) / 2,
              (portrait.height - size) / 2,
              size,
              size,
              0,
              0,
              128,
              128,
            );
            control.append(image);
          } else
            control.textContent = (names[index] ?? `Player ${index + 1}`).slice(
              0,
              1,
            );
          const label =
            index + 1 === viewer
              ? "your board"
              : `${names[index] ?? `Player ${index + 1}`}'s board`;
          control.title = `View ${label}`;
          control.setAttribute("aria-label", control.title);
          const caption = doc.createElement("span");
          caption.textContent =
            index + 1 === viewer
              ? "You"
              : (names[index] ?? `Player ${index + 1}`);
          control.append(caption);
          control.addEventListener("click", () => choose(index + 1), {
            signal: lifetime.signal,
          });
          root.append(control);
          controls.push(control);
        });
      }
      controls.forEach((control, index) => {
        const seat = seats[index];
        control.hidden = !active || blocked || !seat;
        if (seat) {
          const point = project(seat);
          control.style.left = `${point.x}px`;
          control.style.top = `${point.y}px`;
        }
      });
    },
    dispose(): void {
      lifetime.abort();
      root.remove();
    },
  };
}
