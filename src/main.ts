import { mount } from "svelte";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./app.css";
import App from "./App.svelte";

document.addEventListener("contextmenu", (e) => e.preventDefault());

mount(App, {
  target: document.getElementById("app")!,
});

getCurrentWebview()
  .setZoom(1.1)
  .catch(() => {})
  .then(() => getCurrentWindow().show());
