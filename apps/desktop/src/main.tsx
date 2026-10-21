import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [version, setVersion] = useState("...");
  useEffect(() => {
    invoke<string>("core_version").then(setVersion).catch(() => setVersion("unavailable"));
  }, []);
  return (
    <main style={{ fontFamily: "sans-serif", padding: 24 }}>
      <h1>modman</h1>
      <p>core {version}</p>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
