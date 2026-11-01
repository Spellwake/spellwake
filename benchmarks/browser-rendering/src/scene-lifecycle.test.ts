import { expect, test } from "bun:test";
import { createSceneLifecycle, sceneAssetUrl } from "./scene-lifecycle";

test("mount abort removes listeners and releases resources once, in reverse order", () => {
  const external = new AbortController();
  const life = createSceneLifecycle(external.signal);
  const target = new EventTarget();
  const calls: string[] = [];
  target.addEventListener("change", () => calls.push("event"), {
    signal: life.signal,
  });
  life.add(() => calls.push("renderer"));
  life.add(() => calls.push("resources"));
  target.dispatchEvent(new Event("change"));
  external.abort();
  life.dispose();
  target.dispatchEvent(new Event("change"));
  expect(calls).toEqual(["event", "resources", "renderer"]);
  expect(() => life.assertActive()).toThrow();
  life.add(() => calls.push("late texture"));
  expect(calls.at(-1)).toBe("late texture");
});

test("a pre-aborted mount allocates nothing that can survive cleanup", () => {
  const external = new AbortController();
  external.abort();
  const life = createSceneLifecycle(external.signal);
  let disposed = 0;
  life.add(() => disposed++);
  expect(life.disposed).toBe(true);
  expect(disposed).toBe(1);
});

test("asset bases preserve native protocols and nested web paths", () => {
  expect(
    sceneAssetUrl("/cards/magic-back.jpg", "http://localhost/table/"),
  ).toBe("http://localhost/table/cards/magic-back.jpg");
  expect(sceneAssetUrl("fonts/albertsans.ttf", "tauri://localhost/")).toBe(
    "tauri://localhost/fonts/albertsans.ttf",
  );
});
