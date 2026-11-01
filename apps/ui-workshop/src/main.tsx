import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { BoxMotion } from "./routes/BoxMotion";
import { Collection } from "./routes/Collection";

const root = document.getElementById("root");
if (root)
  createRoot(root).render(
    <StrictMode>
      {new URLSearchParams(window.location.search).get("screen") ===
      "box-motion" ? (
        <BoxMotion />
      ) : new URLSearchParams(window.location.search).get("screen") ===
        "collection" ? (
        <Collection />
      ) : (
        <App />
      )}
    </StrictMode>,
  );
