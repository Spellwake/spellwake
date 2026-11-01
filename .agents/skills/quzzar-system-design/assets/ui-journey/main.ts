import { SystemDesigner } from "quzzar-flow";
import "quzzar-flow/style.css";
import { z } from "zod";

// Illustrative requirements only, not defaults for any real product.
const sd = new SystemDesigner({ name: "Profile setup journey" });
const session = z.object({ userId: z.string() });
const profile = z.object({ userId: z.string(), displayName: z.string() });
sd.addGroup({
  id: "screens",
  name: "Profile setup",
  icon: "monitor",
  color: "blue",
});
sd.add({
  id: "sign-in",
  name: "Sign in",
  group: "screens",
  icon: "monitor",
  input: z.object({}),
  output: session,
  processing: "On successful sign-in, the journey continues with the user ID.",
  additionalInfo:
    "Login failure behavior is undecided. Ask the user; no retry or fallback is implied.",
  render: [
    {
      type: "image",
      src: "/screens/sign-in.svg",
      alt: "Sign-in wireframe with email, password, and a sign-in action.",
    },
  ],
});
sd.add({
  id: "profile-empty",
  name: "Profile: empty",
  group: "screens",
  icon: "monitor",
  input: session,
  output: profile,
  processing:
    "Collect a display name before continuing. No picture behavior is decided yet.",
  additionalInfo:
    "Ask what happens with no picture, a supplied picture, or an oversized upload.",
  render: [
    {
      type: "image",
      src: "/screens/profile-empty.svg",
      alt: "Empty profile wireframe with a blank name and no picture.",
    },
  ],
});
sd.add({
  id: "profile-saved",
  name: "Profile: saved",
  group: "screens",
  icon: "monitor",
  input: profile,
  output: profile,
  processing:
    "Show the saved profile. Long-name handling must be agreed with the user.",
  render: [
    {
      type: "image",
      src: "/screens/profile-saved.svg",
      alt: "Saved profile wireframe showing a name and an edit action.",
    },
    {
      type: "json",
      title: "Boundary input to discuss",
      value: {
        displayName: "Alexandra ".repeat(20).trim(),
        pictureBytes: 10_000_001,
      },
    },
    {
      type: "text",
      title: "Open decisions",
      text: "How should a very long name display? Is there an upload cap, and what happens above it? The example supplies no contingency.",
    },
  ],
});
sd.connect("sign-in", "profile-empty", { label: "Sign-in succeeds" });
sd.connect("profile-empty", "profile-saved", { label: "Save profile" });

const validation = sd.validate();
if (!validation.valid) throw new Error(JSON.stringify(validation.issues));
const header = document.createElement("span");
header.textContent = "Profile setup · illustrative journey with open decisions";
const result = await sd.render("diagram", { header });
if (result.status === "error") throw new Error(result.message);
