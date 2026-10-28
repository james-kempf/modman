// SPDX-License-Identifier: GPL-3.0-or-later
// Settings: Nexus account (API key entry with paste + on-screen keyboard)
// then the app paths, auto-update (status, collapsible last-run log) then
// the Steam shortcut, the active game's backup cleanup and restage, config
// keys. Compact rows in a single column.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useAccount } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/shell/Osk";
import { ClipboardTextIcon, FloppyDiskIcon, KeyIcon, KeyboardIcon, SignOutIcon } from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Card,
  CardSection,
  Empty,
  ErrorNote,
  ListToggle,
  Row,
  SECTION_PAD,
  Select,
  StatusDot,
  Toggle,
  ViewHeader,
  cx,
  when,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ConfigEntry, RunLogEntry } from "../ipc/types";
import { GameRepairCard } from "./GameRepair";

export function Settings() {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
      <ViewHeader title="Settings" description="Changes are saved by modman as soon as you make them." />
      <AccountCard />
      <AutomationCard />
      <GameRepairCard />
      <ConfigCard />
      <Card title="About">
        <AppInfo />
      </Card>
    </div>
  );
}

/** Nexus account (first section: harnesses read `main section` for the sign-in state) followed by the app paths. */
function AccountCard() {
  const q = useAccount();
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const [changing, setChanging] = useState(false);
  const a = q.data;
  const signedIn = !!a?.key_source;
  return (
    <Card title="Nexus Mods account" intro="Your personal API key, kept in the system keyring (else the modman config file).">
      {q.isError ? (
        <CardSection>
          <ErrorNote>Could not read the account: {errMsg(q.error)}</ErrorNote>
        </CardSection>
      ) : signedIn ? (
        <Row
          title={
            <span className="flex flex-wrap items-center gap-2">
              Signed in{a?.user && ` as ${a.user}`} {a?.premium ? <Badge tone="accent">Premium</Badge> : <Badge>Free</Badge>}
            </span>
          }
          desc={
            <>
              Key from {a?.key_source}
              {a?.quota && ` · ${a.quota}`}
            </>
          }
        >
          <Button size="sm" onClick={() => setChanging((c) => !c)}>
            {changing ? "Keep current key" : "Change key"}
          </Button>
          <Button
            size="sm"
            tone="danger"
            icon={<SignOutIcon aria-hidden size={15} />}
            onClick={async () => {
              const envStill = await run("Sign out", () => ipc.logout(), { key: "Sign out" });
              if (envStill) notify("warn", "NEXUS_API_KEY is still set in the environment.");
            }}
          >
            Sign out
          </Button>
        </Row>
      ) : (
        <Row title="Not signed in" desc="Get a key at nexusmods.com → Site preferences → API keys." />
      )}
      {(!signedIn || changing) && (
        <CardSection>
          <ApiKeyEntry onDone={() => setChanging(false)} />
        </CardSection>
      )}
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
    const r = await run("Sign in", () => ipc.login(k), { key: "Sign in" });
    if (!r) return;
    notify("success", `Signed in as ${r.user}${r.premium ? " (Premium)" : ""}; key stored in ${r.backend === "keyring" ? "the keyring" : r.file}`);
    if (r.keyring_unavailable) notify("warn", `Keyring unavailable: ${r.keyring_unavailable}`);
    onDone?.();
  };

  return (
    <form
      className="grid gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <label htmlFor="nexus-api-key" className="text-[12.5px] font-medium text-zinc-300">
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
        data-osk-submit="Sign in"
        className="input font-mono"
        placeholder="Paste or type your personal API key"
      />
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          icon={<ClipboardTextIcon aria-hidden size={16} />}
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
        <Button
          size="sm"
          icon={<KeyboardIcon aria-hidden size={16} />}
          onClick={() => input.current && openOsk({ input: input.current, label: "Nexus API key" })}
        >
          Keyboard
        </Button>
        <Button
          type="submit"
          size="sm"
          tone="primary"
          busy={busy}
          disabled={!key.trim()}
          className="ml-auto"
          icon={<KeyIcon aria-hidden size={16} />}
        >
          Sign in
        </Button>
      </div>
    </form>
  );
}

/** Auto-update and the Steam shortcut: both about running modman unattended / from Game Mode. */
function AutomationCard() {
  return (
    <Card title="Auto-update and Steam" intro="Background updates, and starting modman from Steam's Game Mode.">
      <AutoUpdate />
      <SteamShortcut />
    </Card>
  );
}

/** Compact key/value facts: one muted line per fact, wrapping into columns. */
function Facts({ items }: { items: readonly (readonly [string, ReactNode])[] }) {
  return (
    <dl className={cx("grid grid-cols-[repeat(auto-fill,minmax(12rem,1fr))] gap-x-4 gap-y-1 text-[12.5px]", SECTION_PAD)}>
      {items.map(([k, v]) => (
        <div key={k} className="flex min-w-0 gap-2">
          <dt className="shrink-0 text-dim">{k}</dt>
          <dd className="min-w-0 font-mono text-[12px] break-words text-zinc-300">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

function SteamShortcut() {
  const q = useQuery({ queryKey: ["steam-shortcut"], queryFn: ipc.steamShortcutStatus });
  const qc = useQueryClient();
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const busy = useBusy("Steam shortcut");
  const s = q.data;
  const on = !!s && s.accounts > 0 && s.added === s.accounts;
  return (
    <div className="rows">
      <Row
        title="Show modman in Steam"
        desc={
          s?.steam_running
            ? "A non-Steam game entry. Steam is running: restart Steam for the change to show up."
            : s && !s.accounts
              ? "No Steam account was found. Start Steam and sign in once first."
              : "A non-Steam game entry; takes effect the next time Steam starts."
        }
      >
        <Toggle
          label={on ? "Steam shortcut on" : "Steam shortcut off"}
          checked={on}
          disabled={!s || !s.accounts}
          busy={busy}
          onChange={async (v) => {
            const r = await run("Steam shortcut", () => ipc.steamShortcutSet(v), { key: "Steam shortcut" });
            if (r) {
              qc.setQueryData(["steam-shortcut"], r);
              notify("success", `${v ? "Added modman to Steam" : "Removed modman from Steam"}. Restart Steam to apply it.`);
            }
          }}
        />
      </Row>
      {q.isError && (
        <CardSection>
          <ErrorNote>Could not read the Steam shortcuts: {errMsg(q.error)}</ErrorNote>
        </CardSection>
      )}
    </div>
  );
}

function AutoUpdate() {
  const q = useQuery({ queryKey: ["autoupdate"], queryFn: () => ipc.autoupdateStatus(20) });
  const run = useRun();
  const busy = useBusy("Auto-update");
  const [showRuns, setShowRuns] = useState(false);
  const s = q.data;
  const on = !!s?.timer.installed;
  const runs = s?.runs ?? [];
  const last = runs[0];
  return (
    <div className="rows">
      <Row
        title="Automatic updates"
        desc={
          <>
            Every {s?.interval_hours || 1} h after a system or game update: updates mods{s?.collections ? " and collections" : ""}{" "}
            and redeploys, skipping running games.
          </>
        }
      >
        <Toggle
          label={on ? "Auto-update on" : "Auto-update off"}
          checked={on}
          disabled={!s}
          busy={busy}
          onChange={(v) =>
            run<unknown>("Auto-update", () => (v ? ipc.autoupdateEnable() : ipc.autoupdateDisable()), { key: "Auto-update" })
          }
        />
      </Row>
      {q.isError && (
        <CardSection>
          <ErrorNote>Could not read the auto-update status: {errMsg(q.error)}</ErrorNote>
        </CardSection>
      )}
      {s && (
        <Facts
          items={[
            ["Timer", s.timer.installed ? (s.timer.active ?? "installed") : "not installed"],
            ["Next run", s.timer.next_elapse ?? "–"],
            ["Last system change", when(s.last_system_change)],
            [
              "Games",
              s.games.length
                ? s.games.map((g) => (
                    <span key={g.domain} className="block">
                      {g.name}: {g.last_failed ? "last run failed" : `ok ${when(g.last_ok)}`}
                    </span>
                  ))
                : "–",
            ],
          ]}
        />
      )}
      <Row
        title="Last runs"
        desc={last ? `Latest ${when(last.started_at)}: ${last.outcome}` : "No auto-update runs recorded yet."}
      >
        {runs.length > 0 && (
          <ListToggle
            name="Auto-update runs"
            count={runs.length}
            open={showRuns}
            onToggle={() => setShowRuns((o) => !o)}
            controls="autoupdate-runs"
          />
        )}
      </Row>
      {showRuns && runs.length > 0 && (
        <CardSection id="autoupdate-runs">
          <RunLog runs={runs} />
        </CardSection>
      )}
    </div>
  );
}

const OUTCOME = { nothing: "zinc", acted: "accent", failed: "rose", skipped: "amber" } as const;

export function RunLog({ runs }: { runs: RunLogEntry[] }) {
  if (!runs.length) return <Empty>No auto-update runs recorded yet.</Empty>;
  return (
    <ul aria-label="Auto-update runs" className="grid gap-1.5">
      {runs.map((r) => (
        <li key={r.id} tabIndex={0} className="grid gap-0.5 rounded-xl border border-white/[0.05] bg-ink px-3 py-2 text-[12.5px]">
          <span className="flex flex-wrap items-center gap-2">
            <StatusDot tone={OUTCOME[r.outcome]} />
            <span className="text-zinc-200">{when(r.started_at)}</span>
            <span className="font-mono text-[11.5px] text-zinc-400">{r.game ?? "all games"}</span>
            <Badge tone={OUTCOME[r.outcome]}>{r.outcome}</Badge>
            {r.forced && <Badge>forced</Badge>}
            {r.dry_run && <Badge>dry run</Badge>}
          </span>
          {r.summary && <span className="block text-zinc-300">{r.summary}</span>}
          {r.reasons.length > 0 && <span className="block text-dim">why: {r.reasons.join(", ")}</span>}
          {r.actions.length > 0 && <span className="block text-dim">did: {r.actions.join(", ")}</span>}
        </li>
      ))}
    </ul>
  );
}

function ConfigCard() {
  const q = useQuery({ queryKey: ["config"], queryFn: ipc.configList });
  return (
    <Card title="Configuration" intro="modman's config keys, as on the command line (modman config).">
      {q.isError ? (
        <CardSection>
          <ErrorNote>Could not read the configuration: {errMsg(q.error)}</ErrorNote>
        </CardSection>
      ) : (
        // Rows divide with a top border; the first row has none.
        <ul className="grid [&>li]:border-t [&>li]:border-white/[0.05] [&>li:first-child]:border-t-0">
          {q.data?.map((e) => (
            // Text fields key on the value too: a saved (or refetched) value resets the field. Switches and
            // dropdowns show the value as given and must stay mounted, so focus stays on them after a change.
            <ConfigRow key={e.kind === "bool" || e.kind === "enum" ? e.key : `${e.key}=${e.value}`} entry={e} />
          ))}
        </ul>
      )}
    </Card>
  );
}

/** Client-side check of a typed value (the backend validates again); null = acceptable. */
export function configError(entry: ConfigEntry, value: string): string | null {
  const v = value.trim();
  if (entry.kind === "number") return /^\d+$/.test(v) ? null : "Enter a whole number.";
  if (entry.kind === "path") return /^(\/|~(\/|$))/.test(v) ? null : "Enter an absolute path (starting with / or ~).";
  return null;
}

const ROW = cx("flex flex-wrap items-center justify-between gap-x-3 gap-y-1", SECTION_PAD);

/** One setting: a dropdown (enum), a switch (bool), else a validated text field with Save. */
function ConfigRow({ entry }: { entry: ConfigEntry }) {
  const run = useRun();
  const notify = useUi((s) => s.notify);
  const busyKey = `Save ${entry.key}`;
  const busy = useBusy(busyKey);
  const save = async (value: string) => {
    const r = await run("Save setting", () => ipc.configSet(entry.key, value), { key: busyKey });
    if (r) {
      notify("success", `${r.key} = ${r.value}`);
      if (r.old_data_left) notify("warn", `Old data is still in ${r.old_data_left}`);
    }
  };
  const title = (
    <>
      <span className="block font-mono text-[13px] text-zinc-100">{entry.key}</span>{" "}
      <span className="block text-[12px] leading-snug text-dim">{entry.description}</span>
    </>
  );
  const label = "min-w-0 flex-[1_1_9rem] wrap-anywhere";
  if (entry.kind === "bool")
    return (
      <li className={ROW}>
        <span className={label}>{title}</span>
        <Toggle
          label={entry.key}
          showLabel={false}
          checked={entry.value === "true"}
          busy={busy}
          onChange={(v) => void save(v ? "true" : "false")}
        />
      </li>
    );
  if (entry.kind === "enum")
    return (
      <li className={ROW}>
        {/* Visible copy of the dropdown's label, which stays the (screen-reader-only) accessible name. */}
        <span aria-hidden className={label}>
          {title}
        </span>
        <Select
          label={title}
          hideLabel
          value={entry.value}
          options={entry.options.map((o) => ({ value: o, label: o }))}
          busy={busy}
          onChange={(v) => void save(v)}
          className="w-44 font-mono"
        />
      </li>
    );
  return <ConfigTextRow entry={entry} title={title} labelClass={label} save={save} busy={busy} />;
}

function ConfigTextRow({
  entry,
  title,
  labelClass,
  save,
  busy,
}: {
  entry: ConfigEntry;
  title: ReactNode;
  labelClass: string;
  save: (v: string) => Promise<void>;
  busy: boolean;
}) {
  const [value, setValue] = useState(entry.value);
  const error = value === entry.value ? null : configError(entry, value);
  const id = `config-${entry.key}`;
  return (
    <li>
      <form
        className={ROW}
        onSubmit={(e) => {
          e.preventDefault();
          if (!error && value !== entry.value) void save(value.trim());
        }}
      >
        <label htmlFor={id} className={labelClass}>
          {title}
        </label>
        <span className="flex min-w-0 flex-[1_1_14rem] items-center gap-2">
          <input
            id={id}
            className="input min-w-0 flex-1 font-mono"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            inputMode={entry.kind === "number" ? "numeric" : undefined}
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? `${id}-err` : undefined}
          />
          <Button
            type="submit"
            size="sm"
            busy={busy}
            disabled={value === entry.value || !!error}
            icon={<FloppyDiskIcon aria-hidden size={15} />}
          >
            Save
          </Button>
        </span>
        {error && (
          <p id={`${id}-err`} className="basis-full text-[12.5px] text-rose-300">
            {error}
          </p>
        )}
      </form>
    </li>
  );
}

/** App version and paths, below the account. */
function AppInfo() {
  const q = useQuery({ queryKey: ["appinfo"], queryFn: ipc.appInfo });
  const i = q.data;
  if (q.isError)
    return (
      <CardSection>
        <ErrorNote>Could not read the app info: {errMsg(q.error)}</ErrorNote>
      </CardSection>
    );
  if (!i) return null;
  return (
    <CardSection>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-0.5 font-mono text-[12px] leading-relaxed text-zinc-400">
        {(
          [
            ["modman", i.version],
            ["config", i.config_dir],
            ["data", i.data_dir],
            ["log", i.log_path],
            ["database", i.db_path],
          ] as const
        ).map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="text-dim">{k}</dt>
            <dd className="break-all">{v}</dd>
          </div>
        ))}
      </dl>
    </CardSection>
  );
}
