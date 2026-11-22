import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { startDisplayScale } from "./lib/display-scale";
import { initializeRuntime } from "./lib/runtime";
import { Application } from "./routes/Application";

startDisplayScale();
void initializeRuntime();
const root = document.getElementById("root");
if (!root) throw new Error("Application root is missing.");
createRoot(root).render(
  <StrictMode>
    <Application />
  </StrictMode>,
);
