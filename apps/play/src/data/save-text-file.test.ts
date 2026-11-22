import { describe, expect, test } from "bun:test";
import {
  saveTextFile,
  textFileName,
  type FileHost,
} from "../lib/save-text-file";

describe("saving a text file", () => {
  test("file names are safe on every system", () => {
    expect(textFileName("Goblins", "deck")).toBe("Goblins.txt");
    expect(textFileName('  a/b\\c:d*e?f"g<h>i|j  ', "deck")).toBe(
      "abcdefghij.txt",
    );
    expect(textFileName("..hidden", "deck")).toBe("hidden.txt");
    expect(textFileName("Trailing dot. ", "deck")).toBe("Trailing dot.txt");
    expect(textFileName("tab\there\u0000", "deck")).toBe("tabhere.txt");
    expect(textFileName(" /:*? ", "deck")).toBe("deck.txt");
    expect(textFileName("Élan’s deck 🐉", "deck")).toBe("Élan’s deck 🐉.txt");
    expect(Array.from(textFileName("x".repeat(400), "deck")).length).toBe(104);
  });

  test("the native app writes to the place the player picked", async () => {
    const written: [string, string][] = [];
    const asked: string[] = [];
    const host: FileHost = {
      kind: "native",
      choosePlace: (name) => {
        asked.push(name);
        return Promise.resolve("/chosen/Goblins.txt");
      },
      write: (place, text) => {
        written.push([place, text]);
        return Promise.resolve();
      },
    };
    expect(await saveTextFile("Goblins.txt", "4 Shock", host)).toBe("saved");
    expect(asked).toEqual(["Goblins.txt"]);
    expect(written).toEqual([["/chosen/Goblins.txt", "4 Shock"]]);
  });

  test("closing the save dialog writes nothing", async () => {
    let wrote = false;
    const host: FileHost = {
      kind: "native",
      choosePlace: () => Promise.resolve(null),
      write: () => {
        wrote = true;
        return Promise.resolve();
      },
    };
    expect(await saveTextFile("a.txt", "x", host)).toBe("cancelled");
    expect(wrote).toBe(false);
  });

  test("a refused write or dialog is reported, not thrown", async () => {
    const refusedWrite: FileHost = {
      kind: "native",
      choosePlace: () => Promise.resolve("/read-only/a.txt"),
      write: () => Promise.reject(new Error("forbidden path")),
    };
    const noDialog: FileHost = {
      kind: "native",
      choosePlace: () => Promise.reject(new Error("not allowed")),
      write: () => Promise.resolve(),
    };
    expect(await saveTextFile("a.txt", "x", refusedWrite)).toBe("failed");
    expect(await saveTextFile("a.txt", "x", noDialog)).toBe("failed");
  });

  test("the web hands the file to the browser's download", async () => {
    const downloads: [string, string][] = [];
    const host: FileHost = {
      kind: "browser",
      download: (name, text) => void downloads.push([name, text]),
    };
    expect(await saveTextFile("a.txt", "x", host)).toBe("started");
    expect(downloads).toEqual([["a.txt", "x"]]);
  });
});
