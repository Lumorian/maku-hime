import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import OverlayPage from "./obs";
import "./styles/tokens.css";
import "./styles/app.css";
import "./styles/wall.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("找不到 #root 挂载点");
}

createRoot(container).render(
  <StrictMode>
    <OverlayPage />
  </StrictMode>,
);
