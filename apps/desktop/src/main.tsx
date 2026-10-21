// SPDX-License-Identifier: GPL-3.0-or-later
// Placeholder shell until the T14 views land: shows the backend version.
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { appInfo } from "./ipc";

function App() {
  const [version, setVersion] = useState("...");
  useEffect(() => {
    appInfo()
      .then((i) => setVersion(i.version))
      .catch(() => setVersion("unavailable"));
  }, []);
  return (
    <main>
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
