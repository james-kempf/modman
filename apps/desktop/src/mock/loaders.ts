// SPDX-License-Identifier: GPL-3.0-or-later
// Demo backend for mod loaders: one SKSE64 card on the first game (Proton),
// the consent preview, a run that streams its log, launch options with two
// Steam accounts, and a loader archive left among the mods. Follows the
// core's contract: install downloads give a pending preview (nothing runs),
// loader_run needs the preview's id and hash, a used or replaced preview is
// gone, and an unverified preview runs only once re-requested with the ack.

import type { InstalledLoader, LaunchState, LoaderCard, LoaderJunk, LoaderPending, Preview, RunReport, SteamAccount } from "../ipc/types";
import type { Args, Handler } from "./fake";

export interface LoaderHost {
  job: <T>(kind: string, body: (id: number) => Promise<T>) => Promise<T>;
  say: (job: number, text: string) => void;
  log: (previewId: string, text: string) => void;
  /** The game a command names, else the active one. */
  game: (a: Args) => string;
}

export const DEMO_LOADER_MOD = 30379;
const VERSION = "2.2.6";
const DESIRED = 'WINEDLLOVERRIDES="dxgi=n,b" %command%';

const accounts: SteamAccount[] = [
  { account_id: 1001, steam_id: "76561197960266729", account_name: "deck", persona_name: "Deck", timestamp: 1_760_000_000, localconfig: "/home/deck/.steam/steam/userdata/1001/config/localconfig.vdf" },
  { account_id: 1002, steam_id: "76561197960266730", account_name: "family", persona_name: "Family", timestamp: 1_750_000_000, localconfig: "/home/deck/.steam/steam/userdata/1002/config/localconfig.vdf" },
];

export function demoPreview(over: Partial<Preview> = {}): Preview {
  return {
    preview_id: "a1b2c3",
    plan_hash: "h-a1b2c3",
    loader_key: "skse",
    op: "install",
    version: VERSION,
    runtime: {
      kind: "proton",
      proton: "/home/deck/.local/share/Steam/steamapps/common/Proton 9.0 (Beta)/proton",
      compat_data: "/home/deck/.local/share/Steam/steamapps/compatdata/489830",
      client_root: "/home/deck/.local/share/Steam",
      app_id: 489830,
      slr: null,
    },
    commands: [
      {
        program: "/home/deck/.local/share/Steam/steamapps/common/Proton 9.0 (Beta)/proton",
        args: ["run", "Z:\\home\\deck\\.local\\share\\vessel\\loaders\\scratch\\a1b2c3\\installer.exe", "--game-path", "Z:\\home\\deck\\Games\\Skyrim"],
        cwd: "/home/deck/.local/share/vessel/loaders/scratch/a1b2c3",
        env: [
          ["PATH", "/usr/bin:/bin"],
          ["HOME", "/home/deck"],
          ["LANG", "C.UTF-8"],
          ["STEAM_COMPAT_DATA_PATH", "/home/deck/.local/share/Steam/steamapps/compatdata/489830"],
          ["STEAM_COMPAT_CLIENT_INSTALL_PATH", "/home/deck/.local/share/Steam"],
        ],
        cmd_line: null,
      },
    ],
    touch_scope: ["skse64_loader.exe", "skse64_*.dll", "Data/Scripts/**"],
    external_side_effects: [],
    provenance: { kind: "verified", origin: { kind: "nexus", domain: "skyrimspecialedition", mod_id: DEMO_LOADER_MOD, file_id: 1 } },
    warnings: [],
    permissions:
      "The installer runs with your user permissions. Vessel limits its environment, working folder and run time, and backs up and tracks the game files it may change, but it is not sandboxed.",
    expires_at: Math.floor(Date.now() / 1000) + 600,
    timeout_secs: 300,
    ...over,
  };
}

/** The installed loaders of a loader_status answer, as mods_list reports them. */
export const installedLoaders = (cards: LoaderCard[]): InstalledLoader[] =>
  cards.flatMap((c) =>
    c.status.state === "installed"
      ? [{ key: c.key, name: c.name, version: c.status.version, nexus_mod_id: c.nexus_mod_id, install_id: c.status.install_id, status: "installed" as const }]
      : [],
  );

export function loaderHandlers(host: LoaderHost, loaderGame: string): Record<string, Handler> {
  let seq = 0;
  let installed: string | null = null;
  let installId = 0;
  let lastRun: number | null = null;
  let runtime: string | null = null;
  const launch: LaunchState = { desired: null, acked: null, applied: null };
  let junk: LoaderJunk[] = [{ mod_id: 9001, uid: "skse-installer", name: "SKSE 2.2.6 installer", loader_key: "skse" }];
  /** Live previews: id -> preview (a run or a re-request uses one up). */
  const previews = new Map<string, Preview>();
  const issue = (over: Partial<Preview>): Preview => {
    const id = `d${(++seq).toString(16).padStart(6, "0")}`;
    const p = demoPreview({ preview_id: id, plan_hash: `h-${id}`, ...over });
    previews.set(id, p);
    return p;
  };
  const ours = (a: Args) => host.game(a) === loaderGame;
  const pending = (op: LoaderPending["op"]): LoaderPending => {
    const p = issue({ op });
    return { loader_key: "skse", preview_id: p.preview_id, plan_hash: p.plan_hash, op, version: VERSION };
  };

  return {
    loader_status: (a: Args): LoaderCard[] =>
      ours(a)
        ? [
            {
              key: "skse",
              name: "SKSE64",
              nexus_mod_id: DEMO_LOADER_MOD,
              status: installed ? { state: "installed", install_id: installId, version: installed } : { state: "not_installed" },
              runtime: (runtime as LoaderCard["runtime"] | null) ?? "proton",
              runtime_override: runtime,
              launch: { ...launch },
              steam_running: false,
              last_run: lastRun,
              recovered: null,
            },
          ]
        : [],
    /** Pending preview for a loader download (what install returns for the loader's Nexus page). */
    demo_loader_install: (): LoaderPending => pending(installed ? "upgrade" : "install"),
    loader_preview: (a: Args) => {
      if (!installed && (a!.op === "repair" || a!.op === "uninstall")) throw { message: "loader 'skse' is not installed", code: "loader_not_installed" };
      return issue({ op: a!.op as Preview["op"], ...(a!.archive ? { provenance: { kind: "unverified", origin: { kind: "local", path: String(a!.archive) }, reason: "a local file" } } : {}) });
    },
    loader_preview_reopen: (a: Args) => {
      const old = previews.get(String(a!.preview_id));
      if (!old) throw { message: "consent check failed: unknown preview; preview the operation again", code: "loader_consent" };
      previews.delete(old.preview_id);
      return issue({ op: old.op, provenance: old.provenance });
    },
    loader_run: (a: Args) =>
      host.job("loader_run", async (id): Promise<RunReport> => {
        const p = previews.get(String(a!.preview_id));
        if (!p || p.plan_hash !== a!.plan_hash) throw { message: "consent check failed: the plan does not match what was confirmed; preview the operation again", code: "loader_consent" };
        previews.delete(p.preview_id);
        for (const line of ["Preparing install...", `Installing SKSE64 ${VERSION} into ~/Games/Skyrim`, "Done."]) {
          await new Promise((r) => setTimeout(r, 150));
          host.log(p.preview_id, `${line}\n`);
        }
        lastRun = (lastRun ?? 0) + 1;
        if (p.op === "uninstall") {
          installed = null;
          launch.desired = null;
        } else {
          installed = VERSION;
          installId ||= 1;
          launch.desired = DESIRED;
        }
        host.say(id, `${p.op} SKSE64 finished`);
        return {
          install_id: installId,
          run_id: lastRun,
          outcome: { kind: "succeeded" },
          status: installed ? "installed" : "uninstalled",
          version: installed,
          log_dir: "/home/deck/.local/share/vessel/loaders/runs/demo",
          warnings: [],
          launch_desired: launch.desired,
        };
      }),
    loader_rollback: () => [],
    loader_decline_rollback: () => null,
    game_runtime_set: (a: Args) => {
      runtime = (a!.runtime as string | null) ?? null;
      return null;
    },
    launch_options_get: () => ({ ...launch }),
    launch_options_ack: () => (launch.acked = launch.desired),
    launch_options_apply: (a: Args) => {
      if (a!.account == null) return { kind: "choose_account", accounts, default: accounts[0].account_id };
      const acct = accounts.find((x) => x.account_id === a!.account);
      if (!acct) throw { message: `unknown Steam account ${String(a!.account)}`, code: "launch_unknown_account" };
      launch.applied = launch.desired;
      return { kind: "applied", account_id: acct.account_id, path: acct.localconfig, value: launch.desired ?? "", backup: null };
    },
    loader_junk_scan: (a: Args) => (ours(a) ? junk.map((j) => ({ ...j })) : []),
    loader_junk_remove: (a: Args) =>
      host.job("loader_junk_remove", async () => {
        const ids = (a!.mod_ids as number[]) ?? [];
        const before = junk.length;
        junk = junk.filter((j) => !ids.includes(j.mod_id));
        return before - junk.length;
      }),
    loader_log_get: () => `Preparing install...\nInstalling SKSE64 ${VERSION} into ~/Games/Skyrim\nDone.\n`,
  };
}
