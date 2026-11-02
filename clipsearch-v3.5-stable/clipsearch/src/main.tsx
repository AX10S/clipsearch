import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// Mount the launcher. `react-dom/client` is the React 18+ API.
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
