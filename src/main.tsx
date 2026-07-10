import React from "react";
import ReactDOM from "react-dom/client";
// entry point
import App from "./App";
import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/app.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
