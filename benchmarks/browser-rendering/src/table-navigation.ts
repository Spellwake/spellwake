/** Viewer-local controls drawn flat on the table and picked through the scene ray. */
import * as THREE from "three";
export type TableDirection = {
  seat: number;
  angle: number;
  horizontal: number;
  vertical: number;
  offset: number;
};
/** Preserve every seat, including collinear boards; separate controls within each bearing. */
export function navigationDirections(
  seats: readonly { x: number; z: number }[],
  focus: number,
  yaw: number,
): TableDirection[] {
  const origin = seats[focus - 1];
  if (!origin) return [];
  const targets = seats
    .flatMap((seat, index) => {
      if (index + 1 === focus) return [];
      const dx = seat.x - origin.x,
        dz = seat.z - origin.z;
      const x = dx * Math.cos(yaw) - dz * Math.sin(yaw);
      const z = dx * Math.sin(yaw) + dz * Math.cos(yaw);
      return [
        {
          seat: index + 1,
          angle: Math.atan2(x, -z),
          horizontal: Math.abs(x) < 0.01 ? 0 : Math.sign(x),
          vertical: Math.abs(z) < 0.01 ? 0 : Math.sign(z),
          offset: 0,
        },
      ];
    })
    .sort((a, b) => a.angle - b.angle || a.seat - b.seat);
  for (const target of targets) {
    const peers = targets.filter(
      (other) =>
        other.horizontal === target.horizontal &&
        other.vertical === target.vertical,
    );
    target.offset = (peers.indexOf(target) - (peers.length - 1) / 2) * 1.4;
  }
  return targets;
}
export function createTableNavigation(
  surfaceY: number,
  portraits: readonly THREE.Texture[] = [],
) {
  const group = new THREE.Group();
  const arrows = new THREE.Group();
  group.add(arrows);
  const bodyMaterial = new THREE.MeshStandardMaterial({
    color: 0x41493e,
    roughness: 0.9,
  });
  const discGeometry = new THREE.CylinderGeometry(0.41, 0.41, 0.025, 48);
  function inkTexture(label: string | null): THREE.CanvasTexture {
    const canvas = document.createElement("canvas");
    canvas.width = label ? 768 : 128;
    canvas.height = label ? 144 : 128;
    const context = canvas.getContext("2d");
    if (context) {
      if (label) {
        context.fillStyle = "#3b433a";
        context.fillRect(0, 0, canvas.width, canvas.height);
      }
      context.strokeStyle = context.fillStyle = "#eee8d9";
      if (label) {
        context.fillStyle = "#c7c7b5";
        context.font = '400 44px "Albert Sans"';
        context.textAlign = "center";
        context.textBaseline = "middle";
        context.fillText(label, 384, 72, 720);
      } else {
        context.lineWidth = 5;
        context.lineCap = "round";
        context.lineJoin = "round";
        context.beginPath();
        context.moveTo(64, 94);
        context.lineTo(64, 33);
        context.moveTo(39, 58);
        context.lineTo(64, 33);
        context.lineTo(89, 58);
        context.stroke();
      }
    }
    const texture = new THREE.CanvasTexture(canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    return texture;
  }
  const arrowMaterial = new THREE.MeshBasicMaterial({
    map: inkTexture(null),
    toneMapped: false,
    transparent: true,
    depthWrite: false,
  });
  const returnMaterial = new THREE.MeshBasicMaterial({
    map: inkTexture("Return to your board"),
    toneMapped: false,
  });
  const arrowFace = new THREE.CircleGeometry(0.38, 48);
  const faceShape = new THREE.Shape();
  const faceWidth = 4.12,
    faceHeight = 0.68,
    radius = 0.1;
  for (const [x, y, start] of [
    [faceWidth / 2 - radius, faceHeight / 2 - radius, 0],
    [-faceWidth / 2 + radius, faceHeight / 2 - radius, Math.PI / 2],
    [-faceWidth / 2 + radius, -faceHeight / 2 + radius, Math.PI],
    [faceWidth / 2 - radius, -faceHeight / 2 + radius, Math.PI * 1.5],
  ] as const)
    faceShape.absarc(x, y, radius, start, start + Math.PI / 2, false);
  faceShape.closePath();
  const returnFace = new THREE.ShapeGeometry(faceShape);
  const uv = returnFace.getAttribute("uv");
  for (let i = 0; i < uv.count; i++)
    uv.setXY(i, uv.getX(i) / faceWidth + 0.5, uv.getY(i) / faceHeight + 0.5);
  const returnControl = new THREE.Group();
  const returnBody = new THREE.Mesh(
    new THREE.ExtrudeGeometry(faceShape, { depth: 0.025, bevelEnabled: false })
      .scale(4.2 / faceWidth, 0.76 / faceHeight, 1)
      .translate(0, 0, -0.0125)
      .rotateX(-Math.PI / 2),
    bodyMaterial.clone(),
  );
  const returnPrint = new THREE.Mesh(returnFace, returnMaterial);
  returnPrint.rotation.x = -Math.PI / 2;
  returnPrint.position.y = 0.014;
  returnControl.add(returnBody, returnPrint);
  group.add(returnControl);
  type Feedback = {
    control: THREE.Group;
    print: THREE.MeshBasicMaterial;
    body: THREE.MeshStandardMaterial;
    amount: number;
    portrait?: THREE.Texture;
    overlay?: THREE.Group;
    shade?: THREE.MeshBasicMaterial;
    image?: THREE.MeshBasicMaterial;
  };
  const feedback: Feedback[] = [
    {
      control: returnControl,
      print: returnMaterial,
      body: returnBody.material,
      amount: 0,
    },
  ];
  let hovered: THREE.Object3D | null = null;
  let blocked = false;
  let attackMode = false;
  let selectedSeat: number | null = null;
  const idleInk = new THREE.Color(0xc3c8b9),
    activeInk = new THREE.Color(0xffffff);
  const idleLabel = new THREE.Color(0xf1f1e9);
  const idleBody = new THREE.Color(0x41493e),
    activeBody = new THREE.Color(0x77765a);
  function hitControl(ray: THREE.Raycaster): THREE.Object3D | null {
    if (!group.visible || blocked) return null;
    const roots = [arrows, returnControl].filter((root) => root.visible);
    const hit = ray.intersectObjects(roots, true)[0];
    let node: THREE.Object3D | null = hit?.object ?? null;
    while (node && typeof node.userData.navigateSeat !== "number")
      node = node.parent;
    return node;
  }
  const axis = new THREE.Vector3(0, 1, 0);
  return {
    group,
    dispose(): void {
      arrowMaterial.map?.dispose();
      returnMaterial.map?.dispose();
      const geometries = new Set<THREE.BufferGeometry>([
        discGeometry,
        arrowFace,
        returnFace,
      ]);
      const materials = new Set<THREE.Material>([
        arrowMaterial,
        bodyMaterial,
        returnMaterial,
      ]);
      group.traverse((part) => {
        if (part instanceof THREE.Mesh) {
          geometries.add(part.geometry);
          for (const material of Array.isArray(part.material)
            ? part.material
            : [part.material])
            materials.add(material);
        }
      });
      for (const geometry of geometries) geometry.dispose();
      for (const material of materials) material.dispose();
      group.removeFromParent();
      group.clear();
    },
    rebuild(
      origin: { x: number; z: number },
      yaw: number,
      targets: readonly TableDirection[],
      homeSeat: number,
      showReturn: boolean,
    ): void {
      hovered = null;
      for (const state of feedback.slice(1)) {
        state.print.dispose();
        state.body.dispose();
        state.shade?.dispose();
        state.image?.dispose();
      }
      feedback.splice(1);
      arrows.clear();
      for (const target of targets) {
        const control = new THREE.Group();
        const body = new THREE.Mesh(discGeometry, bodyMaterial.clone());
        const print = new THREE.Mesh(arrowFace, arrowMaterial.clone());
        print.rotation.x = -Math.PI / 2;
        print.position.y = 0.018;
        print.rotation.z = -target.angle;
        const overlay = new THREE.Group();
        overlay.add(print);
        control.add(body, overlay);
        const portrait = portraits[target.seat - 1];
        let image: THREE.MeshBasicMaterial | undefined;
        let shade: THREE.MeshBasicMaterial | undefined;
        if (portrait) {
          image = new THREE.MeshBasicMaterial({
            map: portrait,
            toneMapped: false,
          });
          const face = new THREE.Mesh(arrowFace, image);
          face.rotation.x = -Math.PI / 2;
          face.position.y = 0.014;
          control.add(face);
          shade = new THREE.MeshBasicMaterial({
            color: 0x202322,
            transparent: true,
            opacity: 0.52,
            depthWrite: false,
            toneMapped: false,
          });
          const tint = new THREE.Mesh(arrowFace, shade);
          tint.rotation.x = -Math.PI / 2;
          tint.position.y = 0.016;
          overlay.add(tint);
        }
        control.position
          .set(
            target.horizontal * (target.vertical === 0 ? 8 : 6.2) +
              (target.vertical === 0 ? 0 : target.offset),
            0,
            (target.vertical < 0 ? -4.8 : -3.4) +
              (target.vertical === 0 ? target.offset : 0),
          )
          .applyAxisAngle(axis, yaw);
        control.position.add(
          new THREE.Vector3(origin.x, surfaceY + 0.025, origin.z),
        );
        control.rotation.y = yaw;
        control.userData.navigateSeat = target.seat;
        feedback.push({
          control,
          print: print.material,
          body: body.material,
          amount: 0,
          ...(portrait && image && shade
            ? { portrait, overlay, image, shade }
            : {}),
        });
        arrows.add(control);
      }
      returnControl.position
        .set(0, 0, -2.4)
        .applyAxisAngle(axis, yaw)
        .add(new THREE.Vector3(origin.x, surfaceY + 0.025, origin.z));
      returnControl.rotation.y = yaw;
      returnControl.userData.navigateSeat = homeSeat;
      returnControl.visible = showReturn;
    },
    setAttackMode(enabled: boolean, selected: number | null): void {
      attackMode = enabled;
      selectedSeat = selected;
      for (const state of feedback.slice(1)) {
        if (!state.portrait) continue;
        if (state.overlay) state.overlay.visible = !enabled;
      }
    },
    setVisible(
      visible: boolean,
      travelling: boolean,
      interactionBlocked = false,
    ): void {
      group.visible = visible;
      arrows.visible = !travelling;
      blocked = interactionBlocked || travelling;
      if (!visible || blocked) hovered = null;
    },
    hover(ray: THREE.Raycaster | null): boolean {
      const next = ray ? hitControl(ray) : null;
      if (next === hovered) return false;
      hovered = next;
      return true;
    },
    tick(seconds: number, reduced: boolean): boolean {
      let moving = false;
      const blend = reduced ? 1 : 1 - Math.exp(-seconds * 22);
      for (const state of feedback) {
        const target = state.control === hovered ? 1 : 0;
        state.amount = THREE.MathUtils.lerp(state.amount, target, blend);
        state.print.color
          .copy(
            attackMode && state.portrait
              ? activeInk
              : state.control === returnControl
                ? idleLabel
                : idleInk,
          )
          .lerp(activeInk, state.amount);
        state.body.color.copy(idleBody).lerp(activeBody, state.amount);
        if (state.shade) state.shade.opacity = 0.52 - state.amount * 0.14;
        if (attackMode && state.control.userData.navigateSeat === selectedSeat)
          state.body.color.set(0xe7c78d);
        const scale = 1 + state.amount * 0.06;
        state.control.scale.set(scale, 1, scale);
        moving ||= Math.abs(state.amount - target) > 0.001;
      }
      return moving;
    },
    hit(ray: THREE.Raycaster): number | null {
      const node = hitControl(ray);
      return node ? Number(node.userData.navigateSeat) : null;
    },
  };
}
