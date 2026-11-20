/** Parse original JSON only after checking decoded object keys and Unicode.
 * Zod validates the resulting value separately; JSON.parse alone discards duplicates.
 */
export type RawJsonResult =
  { success: true; value: unknown } | { success: false };

/** Invalid wire input is an explicit result and never includes supplied text. */
export function parseStrictJson(
  text: string,
  maximumBytes = 262_144,
): RawJsonResult {
  try {
    return { success: true, value: decodeStrictJson(text, maximumBytes) };
  } catch {
    return { success: false };
  }
}

function decodeStrictJson(text: string, maximumBytes: number): unknown {
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0) {
    throw new Error("Invalid JSON boundary configuration");
  }
  if (new TextEncoder().encode(text).length > maximumBytes) {
    throw new Error("JSON exceeds the wire boundary");
  }
  let at = 0;
  const invalid = (): never => {
    throw new Error("Invalid original JSON wire");
  };
  const space = (): void => {
    while (
      text[at] === " " ||
      text[at] === "\t" ||
      text[at] === "\n" ||
      text[at] === "\r"
    )
      at += 1;
  };
  const string = (): string => {
    if (text[at] !== '"') return invalid();
    const start = at++;
    while (at < text.length) {
      const char = text[at++];
      if (char === '"') {
        const result: unknown = JSON.parse(text.slice(start, at));
        if (typeof result !== "string") return invalid();
        for (let index = 0; index < result.length; index += 1) {
          const code = result.charCodeAt(index);
          if (code >= 0xd800 && code <= 0xdbff) {
            const next = result.charCodeAt(++index);
            if (!(next >= 0xdc00 && next <= 0xdfff)) return invalid();
          } else if (code >= 0xdc00 && code <= 0xdfff) return invalid();
        }
        return result;
      }
      if (char === "\\") {
        const escape = text[at++];
        if (escape === "u") {
          if (!/^[0-9a-fA-F]{4}$/.test(text.slice(at, at + 4)))
            return invalid();
          at += 4;
        } else if (escape === undefined || !'"\\/bfnrt'.includes(escape))
          return invalid();
      } else if (char === undefined || char.charCodeAt(0) < 32)
        return invalid();
    }
    return invalid();
  };
  const numberToken = /-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/y;
  const value = (depth: number): void => {
    if (depth > 64) return invalid();
    space();
    const char = text[at];
    if (char === '"') {
      string();
      return;
    }
    if (char === "{" || char === "[") {
      const object = char === "{";
      const end = object ? "}" : "]";
      const keys = new Set<string>();
      at += 1;
      space();
      if (text[at] === end) {
        at += 1;
        return;
      }
      while (true) {
        if (object) {
          const key = string();
          if (keys.has(key)) return invalid();
          keys.add(key);
          space();
          if (text[at++] !== ":") return invalid();
        }
        value(depth + 1);
        space();
        if (text[at] === end) {
          at += 1;
          return;
        }
        if (text[at++] !== ",") return invalid();
        space();
      }
    }
    for (const literal of ["true", "false", "null"]) {
      if (text.startsWith(literal, at)) {
        at += literal.length;
        return;
      }
    }
    numberToken.lastIndex = at;
    const number = numberToken.exec(text);
    if (!number) return invalid();
    at += number[0].length;
  };
  value(0);
  space();
  if (at !== text.length) return invalid();
  const result: unknown = JSON.parse(text);
  return result;
}
