// SPDX-License-Identifier: GPL-3.0-or-later
// Test harness: a fake transport with sane defaults, fresh store/bridge,
// and the full App (or any element) rendered inside a QueryClient.

import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import { App } from "../app/App";
import { resetBridge, startBridge } from "../app/bridge";
import { makeQueryClient } from "../app/queries";
import { resetUi } from "../app/store";
import { setTransport } from "../ipc/transport";
import type { GameView } from "../ipc/types";
import { createFakeTransport, type Handler } from "../mock/fake";
import { autoupdateStatus, game } from "../mock/fixtures";

export function baseHandlers(games: GameView[] = [game()]): Record<string, Handler> {
  return {
    app_ready: () => ({ startup_check: null, queued: 0 }),
    app_info: () => ({
      version: "test",
      config_dir: "/c",
      data_dir: "/d",
      log_path: "/l",
      db_path: "/db",
      legacy_state_dir: null,
    }),
    games_list: () => games,
    games_detect: () => [],
    mods_list: () => ({ mods: [], changed_since_deploy: false }),
    account: () => ({ key_source: null, user: null, premium: null, quota: null }),
    config_list: () => [],
    autoupdate_status: () => autoupdateStatus(),
    log_tail: () => [],
    downloads_list: () => [],
    deps: () => [],
    collections_list: () => [],
    conflicts: () => ({ files: [], plugins: [] }),
    profiles_list: () => [],
    plugins_list: () => [],
    thumbnail: () => null,
    game_art: () => null,
    collection_art: () => null,
    prompt_answer: () => true,
  };
}

export async function setup(handlers: Record<string, Handler> = {}, games?: GameView[]) {
  resetUi();
  resetBridge();
  const t = createFakeTransport({ ...baseHandlers(games), ...handlers });
  setTransport(t);
  const qc = makeQueryClient();
  await startBridge(qc);
  const renderIn = (ui: ReactElement) => render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
  return { t, qc, renderIn, renderApp: () => renderIn(<App />) };
}
