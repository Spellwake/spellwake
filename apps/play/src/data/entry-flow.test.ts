import { describe, expect, test } from "bun:test";
import {
  emailProblem,
  registrationProblem,
  usernameProblem,
} from "../auth/entry-flow";

describe("account entry rules", () => {
  test("an email needs a name, a host and a dot, within 254 characters", () => {
    expect(emailProblem("mage@example.com")).toBeNull();
    expect(emailProblem("  mage@example.com  ")).toBeNull();
    for (const bad of [
      "",
      "   ",
      "mage@",
      "mage",
      "@example.com",
      "a b@c.d",
      "mage@example",
      "mage@example..com",
    ])
      expect(emailProblem(bad)).toBe("Enter a valid email address.");
    expect(emailProblem(`${"a".repeat(250)}@b.co`)).toBe(
      "Enter a valid email address.",
    );
  });

  test("a username is 3 to 24 letters, numbers or underscores", () => {
    expect(usernameProblem("Moonwatcher")).toBeNull();
    expect(usernameProblem("a_1")).toBeNull();
    for (const bad of [
      "",
      "ab",
      "moon watcher",
      "a".repeat(25),
      "né_name",
      " moon",
    ])
      expect(usernameProblem(bad)).toBe(
        "Use 3–24 letters, numbers or underscores.",
      );
  });

  test("the server's registration answers map to the approved copy", () => {
    expect(registrationProblem({ username: "unavailable" })).toEqual({
      field: "That username is taken.",
    });
    expect(registrationProblem({ username: "invalid" })).toEqual({
      field: "Use 3–24 letters, numbers or underscores.",
    });
    expect(registrationProblem({ account: "unavailable" })).toEqual({
      notice: "Couldn’t create account.",
    });
    expect(registrationProblem({})).toEqual({
      notice: "Couldn’t create account.",
    });
  });
});
