import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./ui/theme.css";
import "./ui/settings.css";
import "./ui/home.css";
import "./ui/generate.css";
import "./ui/reader.css";
import "./ui/digest.css";
import "./ui/sync.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
