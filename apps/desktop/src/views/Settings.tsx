// SPDX-License-Identifier: GPL-3.0-or-later
// Settings: Nexus account (API key entry with paste + on-screen keyboard),
// auto-update toggle with status and last-run log, config keys, app paths.

import { useQuery } from "@tanstack/react-query";
import { useRef, useState, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { useAccount } from "../app/queries";
import { useBusy, useRun } from "../app/run";
import { useUi } from "../app/store";
import { readClipboard } from "../components/Osk";
import { ClipboardTextIcon, FloppyDiskIcon, KeyIcon, KeyboardIcon, SignOutIcon } from "@phosphor-icons/react";
import {
  Badge,
  Button,
  Empty,
  ErrorNote,
  Fact,
  Row,
  Section,
  Select,
  StatusDot,
  Toggle,
  ViewHeader,
  when,
} from "../components/ui";
import * as ipc from "../ipc/commands";
import type { ConfigEntry, RunLogEntry } from "../ipc/types";

export function Settings() {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)] gap-14">
      <ViewHeader title="Settings" description="Changes are saved by modman as soon as you make them." />
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
    <Section
      title="Nexus Mods account"
      intro="modman signs in with your personal API key. It is kept in the system keyring, or the modman config file when there is none."
    >
      {q.isError ? (
        <div className="px-6 py-5">
          <ErrorNote>Could not read the account: {errMsg(q.error)}</ErrorNote>
        </div>
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
        <Row title="Not signed in" desc="Get a personal API key at nexusmods.com → Site preferences → API keys." />
      )}
      {(!signedIn || changing) && (
        <div className="px-6 py-5">
          <ApiKeyEntry onDone={() => setChanging(false)} />
        </div>
      )}
    </Section>
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
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <label htmlFor="nexus-api-key" className="text-[13px] font-medium text-zinc-300">
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
      <div className="flex flex-wrap gap-3">
        <Button
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
          icon={<KeyboardIcon aria-hidden size={16} />}
          onClick={() => input.current && openOsk({ input: input.current, label: "Nexus API key" })}
        >
          Keyboard
        </Button>
        <Button
          type="submit"
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

function AutoUpdateCard() {
  const q = useQuery({ queryKey: ["autoupdate"], queryFn: () => ipc.autoupdateStatus(20) });
  const run = useRun();
  const busy = useBusy("Auto-update");
  const s = q.data;
  const on = !!s?.timer.installed;
  return (
    <Section
      title="Auto-update"
      intro="After an OS update or a Steam update of a game, modman installs mod updates and redeploys every game that has mods deployed. It waits while a game is running."
    >
      <Row
        title="Automatic updates"
        desc={
          <>
            A systemd user timer runs modman every {s?.interval_hours || 1} h: after a system or game update it updates mods
            {s?.collections ? " and collections" : ""} and redeploys, skipping games that are running or updating.
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
        <div className="px-6 py-5">
          <ErrorNote>Could not read the auto-update status: {errMsg(q.error)}</ErrorNote>
        </div>
      )}
      {s && (
        <dl className="grid grid-cols-2 gap-x-6 gap-y-4 px-6 py-5 md:grid-cols-4">
          <Fact k="Timer" v={s.timer.installed ? (s.timer.active ?? "installed") : "not installed"} />
          <Fact k="Next run" v={s.timer.next_elapse ?? "–"} />
          <Fact k="Last system change" v={when(s.last_system_change)} />
          <Fact
            k="Games"
            v={s.games.map((g) => (
              <span key={g.domain} className="block">
                {g.name}: {g.last_failed ? "last run failed" : `ok ${when(g.last_ok)}`}
              </span>
            ))}
          />
        </dl>
      )}
      <div className="px-6 py-5">
        <h3 className="eyebrow mb-3">Last runs</h3>
        <RunLog runs={s?.runs ?? []} />
      </div>
    </Section>
  );
}

const OUTCOME = { nothing: "zinc", acted: "accent", failed: "rose", skipped: "amber" } as const;

export function RunLog({ runs }: { runs: RunLogEntry[] }) {
  if (!runs.length) return <Empty>No auto-update runs recorded yet.</Empty>;
  return (
    <ul aria-label="Auto-update runs" className="grid gap-2">
      {runs.map((r) => (
        <li key={r.id} tabIndex={0} className="grid gap-1 rounded-2xl border border-white/[0.05] bg-ink px-4 py-3 text-[13px]">
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
    <Section title="Configuration" intro="modman's config keys, as on the command line (modman config).">
      {q.isError ? (
        <div className="px-6 py-5">
          <ErrorNote>Could not read the configuration: {errMsg(q.error)}</ErrorNote>
        </div>
      ) : (
        <ul className="rows">
          {q.data?.map((e) => (
            // Text fields key on the value too: a saved (or refetched) value resets the field. Switches and
            // dropdowns show the value as given and must stay mounted, so focus stays on them after a change.
            <ConfigRow key={e.kind === "bool" || e.kind === "enum" ? e.key : `${e.key}=${e.value}`} entry={e} />
          ))}
        </ul>
      )}
    </Section>
  );
}

/** Client-side check of a typed value (the backend validates again); null = acceptable. */
export function configError(entry: ConfigEntry, value: string): string | null {
  const v = value.trim();
  if (entry.kind === "number") return /^\d+$/.test(v) ? null : "Enter a whole number.";
  if (entry.kind === "path") return /^(\/|~(\/|$))/.test(v) ? null : "Enter an absolute path (starting with / or ~).";
  return null;
}

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
      <span className="font-mono text-zinc-100">{entry.key}</span> <span className="text-dim">{entry.description}</span>
    </>
  );
  if (entry.kind === "bool")
    return (
      <li className="flex flex-wrap items-center justify-between gap-3 px-6 py-4">
        <span className="min-w-0 flex-[1_1_18rem] text-[13px] wrap-anywhere text-zinc-300">{title}</span>
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
      <li className="px-6 py-5">
        <Select
          label={title}
          value={entry.value}
          options={entry.options.map((o) => ({ value: o, label: o }))}
          busy={busy}
          onChange={(v) => void save(v)}
          className="max-w-md font-mono"
        />
      </li>
    );
  return <ConfigTextRow entry={entry} title={title} save={save} busy={busy} />;
}

function ConfigTextRow({
  entry,
  title,
  save,
  busy,
}: {
  entry: ConfigEntry;
  title: ReactNode;
  save: (v: string) => Promise<void>;
  busy: boolean;
}) {
  const [value, setValue] = useState(entry.value);
  const error = value === entry.value ? null : configError(entry, value);
  const id = `config-${entry.key}`;
  return (
    <li className="px-6 py-5">
      <form
        className="grid gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!error && value !== entry.value) void save(value.trim());
        }}
      >
        <label htmlFor={id} className="text-[13px] wrap-anywhere text-zinc-300">
          {title}
        </label>
        <div className="flex flex-wrap items-center gap-3">
          <input
            id={id}
            className="input min-w-0 flex-[1_1_18rem] font-mono"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            inputMode={entry.kind === "number" ? "numeric" : undefined}
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? `${id}-err` : undefined}
          />
          <Button
            type="submit"
            busy={busy}
            disabled={value === entry.value || !!error}
            icon={<FloppyDiskIcon aria-hidden size={15} />}
          >
            Save
          </Button>
        </div>
        {error && (
          <p id={`${id}-err`} className="text-[13px] text-rose-300">
            {error}
          </p>
        )}
      </form>
    </li>
  );
}

function AppInfoCard() {
  const q = useQuery({ queryKey: ["appinfo"], queryFn: ipc.appInfo });
  const i = q.data;
  if (q.isError)
    return (
      <Section title="About">
        <div className="px-6 py-5">
          <ErrorNote>Could not read the app info: {errMsg(q.error)}</ErrorNote>
        </div>
      </Section>
    );
  if (!i) return null;
  return (
    <Section title="About">
      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 px-6 py-5 font-mono text-[12px] leading-relaxed text-zinc-400">
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
    </Section>
  );
}
