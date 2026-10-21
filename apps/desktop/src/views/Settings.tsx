// SPDX-License-Identifier: GPL-3.0-or-later
// Settings: Nexus account (API key entry with paste + on-screen keyboard),
// auto-update toggle with status and last-run log, config keys, app paths.

import { useQuery } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useAccount } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/Osk";
import { Badge, Button, Card, Empty, Toggle, ViewHeader, when } from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ConfigEntry, RunLogEntry } from "../ipc/types";

export function Settings() {
  return (
    <div className="grid gap-6">
      <ViewHeader title="Settings" />
      <AccountCard />
      <AutoUpdateCard />
      <ConfigCard />
      <AppInfoCard />
    </div>
  );
}

function AccountCard() {
  const q = useAccount();
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const [changing, setChanging] = useState(false);
  const a = q.data;
  const signedIn = !!a?.key_source;
  return (
    <Card
      title="Nexus Mods account"
      actions={
        signedIn && (
          <>
            <Button onClick={() => setChanging((c) => !c)}>{changing ? "Keep current key" : "Change key"}</Button>
            <Button
              tone="danger"
              onClick={async () => {
                const envStill = await run("Sign out", () => ipc.logout());
                if (envStill) notify("warn", "NEXUS_API_KEY is still set in the environment.");
              }}
            >
              Sign out
            </Button>
          </>
        )
      }
    >
      {signedIn ? (
        <p className="mb-4 text-zinc-300">
          Signed in{a?.user && ` as ${a.user}`} {a?.premium ? <Badge tone="accent">Premium</Badge> : <Badge>Free</Badge>}
          <span className="block text-sm text-zinc-500">
            Key from {a?.key_source}
            {a?.quota && ` · ${a.quota}`}
          </span>
        </p>
      ) : (
        <p className="mb-4 text-zinc-400">
          Not signed in. Get a personal API key at nexusmods.com → Site preferences → API keys.
        </p>
      )}
      {(!signedIn || changing) && <ApiKeyEntry onDone={() => setChanging(false)} />}
    </Card>
  );
}

/**
 * Key entry for desktop and Deck: Steam's keyboard opens on focus in Game
 * Mode; "Keyboard" opens ours, "Paste" reads the clipboard. The key lives
 * only in this component and is cleared as soon as it is submitted.
 */
export function ApiKeyEntry({ onDone }: { onDone?: () => void }) {
  const [key, setKey] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const openOsk = useUi((s) => s.openOsk);
  const busy = useBusy("Sign in");

  const submit = async () => {
    const k = key.trim();
    if (!k) return;
    setKey("");
    const r = await run("Sign in", () => ipc.login(k));
    if (!r) return;
    notify("success", `Signed in as ${r.user}${r.premium ? " (Premium)" : ""}; key stored in ${r.backend === "keyring" ? "the keyring" : r.file}`);
    if (r.keyring_unavailable) notify("warn", `Keyring unavailable: ${r.keyring_unavailable}`);
    onDone?.();
  };

  return (
    <form
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <label htmlFor="nexus-api-key" className="text-sm text-zinc-400">
        Nexus API key
      </label>
      <input
        id="nexus-api-key"
        ref={input}
        type="password"
        autoComplete="off"
        spellCheck={false}
        value={key}
        onChange={(e) => setKey(e.target.value)}
        className="input font-mono"
        placeholder="Paste or type your personal API key"
      />
      <div className="flex flex-wrap gap-3">
        <Button
          onClick={async () => {
            const t = await readClipboard();
            if (t === null) {
              notify("warn", "Clipboard not readable here; paste into the field with Ctrl+V.");
              input.current?.focus();
            } else setKey(t.trim());
          }}
        >
          Paste
        </Button>
        <Button onClick={() => input.current && openOsk({ input: input.current, label: "Nexus API key" })}>Keyboard</Button>
        <Button type="submit" tone="primary" busy={busy} disabled={!key.trim()} className="ml-auto">
          Sign in
        </Button>
      </div>
    </form>
  );
}

function AutoUpdateCard() {
  const q = useQuery({ queryKey: ["autoupdate"], queryFn: () => ipc.autoupdateStatus(20) });
  const run = useRun();
  const busy = useBusy("Auto-update");
  const s = q.data;
  const on = !!s?.timer.installed;
  return (
    <Card
      title="Automatic updates"
      actions={
        <Toggle
          label={on ? "Auto-update on" : "Auto-update off"}
          checked={on}
          disabled={!s || busy}
          onChange={(v) => run<unknown>("Auto-update", () => (v ? ipc.autoupdateEnable() : ipc.autoupdateDisable()))}
        />
      }
    >
      <p className="text-sm text-zinc-400">
        A systemd user timer runs modman every {s?.interval_hours || 1} h: after a system or game update it updates mods
        {s?.collections ? " and collections" : ""} and redeploys, skipping games that are running or updating.
      </p>
      {s && (
        <dl className="mt-4 grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
          <div>
            <dt className="text-zinc-500">Timer</dt>
            <dd className="text-zinc-200">{s.timer.installed ? (s.timer.active ?? "installed") : "not installed"}</dd>
          </div>
          <div>
            <dt className="text-zinc-500">Next run</dt>
            <dd className="text-zinc-200">{s.timer.next_elapse ?? "–"}</dd>
          </div>
          <div>
            <dt className="text-zinc-500">Last system change</dt>
            <dd className="text-zinc-200">{when(s.last_system_change)}</dd>
          </div>
          <div>
            <dt className="text-zinc-500">Games</dt>
            <dd className="text-zinc-200">
              {s.games.map((g) => (
                <span key={g.domain} className="block">
                  {g.name}: {g.last_failed ? "last run failed" : `ok ${when(g.last_ok)}`}
                </span>
              ))}
            </dd>
          </div>
        </dl>
      )}
      <h3 className="eyebrow mt-6 mb-2">Last runs</h3>
      <RunLog runs={s?.runs ?? []} />
    </Card>
  );
}

const OUTCOME = { nothing: "zinc", acted: "accent", failed: "rose", skipped: "amber" } as const;

export function RunLog({ runs }: { runs: RunLogEntry[] }) {
  if (!runs.length) return <Empty>No auto-update runs recorded yet.</Empty>;
  return (
    <ul aria-label="Auto-update runs" className="grid gap-2">
      {runs.map((r) => (
        <li key={r.id} tabIndex={0} className="rounded-xl border border-white/[0.06] p-3 text-sm">
          <span className="flex flex-wrap items-center gap-2">
            <span className="text-zinc-200">{when(r.started_at)}</span>
            <span className="text-zinc-400">{r.game ?? "all games"}</span>
            <Badge tone={OUTCOME[r.outcome]}>{r.outcome}</Badge>
            {r.forced && <Badge>forced</Badge>}
            {r.dry_run && <Badge>dry run</Badge>}
          </span>
          {r.summary && <span className="block text-zinc-300">{r.summary}</span>}
          {r.reasons.length > 0 && <span className="block text-zinc-500">why: {r.reasons.join(", ")}</span>}
          {r.actions.length > 0 && <span className="block text-zinc-500">did: {r.actions.join(", ")}</span>}
        </li>
      ))}
    </ul>
  );
}

function ConfigCard() {
  const q = useQuery({ queryKey: ["config"], queryFn: ipc.configList });
  return (
    <Card title="Configuration">
      <ul className="grid gap-3">
        {q.data?.map((e) => (
          <ConfigRow key={e.key} entry={e} />
        ))}
      </ul>
    </Card>
  );
}

function ConfigRow({ entry }: { entry: ConfigEntry }) {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const [value, setValue] = useState(entry.value);
  const id = `config-${entry.key}`;
  return (
    <li>
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          const r = await run("Save setting", () => ipc.configSet(entry.key, value));
          if (r) {
            notify("success", `${r.key} = ${r.value}`);
            if (r.old_data_left) notify("warn", `Old data is still in ${r.old_data_left}`);
          }
        }}
      >
        <div className="grid min-w-72 flex-1 gap-1">
          <label htmlFor={id} className="text-sm text-zinc-300">
            <span className="font-mono">{entry.key}</span> <span className="text-zinc-500">{entry.description}</span>
          </label>
          <input id={id} className="input font-mono" value={value} onChange={(e) => setValue(e.target.value)} />
        </div>
        <Button type="submit" disabled={value === entry.value}>
          Save
        </Button>
      </form>
    </li>
  );
}

function AppInfoCard() {
  const q = useQuery({ queryKey: ["appinfo"], queryFn: ipc.appInfo });
  const i = q.data;
  if (!i) return null;
  return (
    <Card title="About">
      <dl className="grid gap-2 font-mono text-sm text-zinc-400">
        <div>modman {i.version}</div>
        <div>config: {i.config_dir}</div>
        <div>data: {i.data_dir}</div>
        <div>log: {i.log_path}</div>
        <div>database: {i.db_path}</div>
      </dl>
    </Card>
  );
}
