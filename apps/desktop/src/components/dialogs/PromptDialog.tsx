// SPDX-License-Identifier: GPL-3.0-or-later
// One dialog per backend question (`vessel:prompt`), answered through
// `prompt_answer`; "back" answers cancel. Questions queue up in order;
// `vessel:prompt-closed` withdraws one. The fomod_group dialog is the
// FOMOD installer wizard (one group per step, re-asked with an error).

import { useState, type ReactNode } from "react";
import { errMsg } from "../../app/bridge";
import { useUi } from "../../app/store";
import { promptAnswer } from "../../ipc/commands";
import type { PromptEvent } from "../../ipc/events";
import type { Answer, FomodOption, GroupType, Prompt } from "../../ipc/types";
import { Dialog } from "./Dialog";
import { CheckIcon, WarningIcon } from "@phosphor-icons/react";
import { Badge, Button, TextField, TILE, bytes, cx } from "../ui";

export function PromptHost() {
  const head = useUi((s) => s.prompts[0]);
  if (!head) return null;
  // key: a fresh dialog (state + focus) per prompt, also for a FOMOD re-ask.
  return <PromptDialog key={head.id} event={head} />;
}

function PromptDialog({ event }: { event: PromptEvent }) {
  const drop = useUi((s) => s.dropPrompt);
  const notify = useUi((s) => s.notify);
  const [sending, setSending] = useState(false);

  const answer = async (a: Answer) => {
    if (sending) return;
    setSending(true);
    try {
      await promptAnswer(event.id, a);
    } catch (e) {
      notify("error", `Answer not delivered: ${errMsg(e)}`);
    } finally {
      // Gone either way: answered, or the backend no longer waits for it.
      drop(event.id);
    }
  };
  const cancel = () => void answer({ type: "cancel" });
  const p = event.prompt;
  const props = { answer, cancel, sending };

  switch (p.type) {
    case "confirm":
      return <Confirm p={p} {...props} />;
    case "text":
      return <Text p={p} {...props} />;
    case "choose_file":
      return <ChooseFile p={p} {...props} />;
    case "choose_optional":
      return <ChooseOptional p={p} {...props} />;
    case "manual_download":
      return <ManualDownload p={p} {...props} />;
    case "choose_variant":
      return <ChooseVariant p={p} {...props} />;
    case "fomod_group":
      return <FomodGroup p={p} {...props} />;
  }
}

type Of<T extends Prompt["type"]> = Extract<Prompt, { type: T }>;
interface Props<T extends Prompt["type"]> {
  p: Of<T>;
  answer: (a: Answer) => void;
  cancel: () => void;
  sending: boolean;
}

function Actions({ children }: { children: ReactNode }) {
  return <div className="mt-7 flex flex-wrap justify-end gap-2">{children}</div>;
}

function Confirm({ p, answer, cancel, sending }: Props<"confirm">) {
  return (
    <Dialog title="Confirm" onBack={cancel}>
      <p className="text-title leading-relaxed text-zinc-200">{p.question}</p>
      <Actions>
        <Button
          data-autofocus={!p.default_yes || undefined}
          disabled={sending}
          onClick={() => answer({ type: "confirm", yes: false })}
        >
          No
        </Button>
        <Button
          tone="primary"
          data-autofocus={p.default_yes || undefined}
          disabled={sending}
          onClick={() => answer({ type: "confirm", yes: true })}
        >
          Yes
        </Button>
      </Actions>
    </Dialog>
  );
}

function Text({ p, answer, cancel, sending }: Props<"text">) {
  const [value, setValue] = useState(p.default);
  return (
    <Dialog title="Input needed" onBack={cancel}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          answer({ type: "text", value });
        }}
      >
        <TextField label={p.question} value={value} onChange={(e) => setValue(e.target.value)} data-autofocus />
        <Actions>
          <Button tone="ghost" onClick={cancel} disabled={sending}>
            Cancel
          </Button>
          <Button type="submit" tone="primary" disabled={sending}>
            OK
          </Button>
        </Actions>
      </form>
    </Dialog>
  );
}

/** A full-width choice row (button, aria-pressed). */
function Choice({
  selected,
  onClick,
  disabled,
  autoFocus,
  children,
  multi,
  onFocus,
}: {
  selected: boolean;
  onClick: () => void;
  onFocus?: () => void;
  disabled?: boolean;
  autoFocus?: boolean;
  children: ReactNode;
  multi?: boolean;
}) {
  return (
    <button
      type="button"
      role={multi ? "checkbox" : "radio"}
      aria-checked={selected}
      disabled={disabled}
      data-autofocus={autoFocus || undefined}
      onClick={onClick}
      onFocus={onFocus}
      className={cx(
        "flex min-h-control-lg w-full items-start gap-3 rounded-card border px-4 py-3 text-left transition-colors duration-base",
        selected
          ? "border-accent-400/40 bg-accent-400/10"
          : "border-hairline bg-fill-1 hover:bg-fill-2",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "mt-0.5 grid size-icon shrink-0 place-items-center border-2 transition-colors duration-base",
          multi ? "rounded-control-sm" : "rounded-pill",
          selected ? "border-accent-400 bg-accent-400 text-accent-950" : "border-zinc-500",
        )}
      >
        {selected && (multi ? <CheckIcon size={12} weight="bold" /> : <span className="size-2 rounded-pill bg-accent-950" />)}
      </span>
      <span className="min-w-0 flex-1">{children}</span>
    </button>
  );
}

function ChooseFile({ p, answer, cancel, sending }: Props<"choose_file">) {
  const [sel, setSel] = useState(p.default);
  return (
    <Dialog title="Choose a file" subtitle="This mod has several files." onBack={cancel}>
      <div role="radiogroup" aria-label="Files" className="grid gap-2">
        {p.files.map((f, i) => (
          <Choice key={f.file_id} selected={sel === i} onClick={() => setSel(i)} autoFocus={i === p.default}>
            <span className="block text-zinc-100">
              {f.name} {f.version && <span className="text-zinc-400">v{f.version}</span>}
            </span>
            <span className="block font-mono text-caption text-dim">
              {[f.category_name, f.file_name, f.size_in_bytes !== null && bytes(f.size_in_bytes)]
                .filter(Boolean)
                .join(" · ")}
              {f.is_primary && " · primary"}
            </span>
          </Choice>
        ))}
      </div>
      <Actions>
        <Button tone="ghost" onClick={cancel} disabled={sending}>
          Cancel
        </Button>
        <Button tone="primary" disabled={sending} onClick={() => answer({ type: "choice", index: sel })}>
          Install this file
        </Button>
      </Actions>
    </Dialog>
  );
}

function ChooseOptional({ p, answer, cancel, sending }: Props<"choose_optional">) {
  const [sel, setSel] = useState<number[]>([]);
  const toggle = (i: number) => setSel((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i].sort((a, b) => a - b)));
  return (
    <Dialog title={`Optional mods in ${p.collection}`} subtitle="Pick the optional mods to install." onBack={cancel}>
      <div className="grid gap-2">
        {p.mods.map((m, i) => (
          <Choice key={i} multi selected={sel.includes(i)} onClick={() => toggle(i)} autoFocus={i === 0}>
            <span className="text-zinc-100">{m.name}</span>{" "}
            <span className="font-mono text-caption text-dim">
              {m.version && `v${m.version} · `}
              {bytes(m.size)}
            </span>
          </Choice>
        ))}
      </div>
      <Actions>
        <Button tone="ghost" onClick={cancel} disabled={sending}>
          Cancel
        </Button>
        <Button onClick={() => setSel(p.mods.map((_, i) => i))}>Select all</Button>
        <Button tone="primary" disabled={sending} onClick={() => answer({ type: "choices", indices: sel })}>
          Install {sel.length} optional
        </Button>
      </Actions>
    </Dialog>
  );
}

function ManualDownload({ p, answer, cancel, sending }: Props<"manual_download">) {
  const [path, setPath] = useState("");
  return (
    <Dialog title={`Manual download: ${p.name}`} subtitle="This file cannot be downloaded automatically." onBack={cancel}>
      {p.instructions && <p className="text-body leading-relaxed whitespace-pre-wrap text-zinc-300">{p.instructions}</p>}
      {p.url && (
        <p className="mt-3 text-body text-zinc-400">
          Download it from <span className="font-mono break-all text-zinc-200 select-all">{p.url}</span>
        </p>
      )}
      <form
        className="mt-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) answer({ type: "path", path: path.trim() });
        }}
      >
        <TextField
          label="Path of the downloaded archive"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="/home/deck/Downloads/archive.7z"
          data-autofocus
        />
        <Actions>
          <Button tone="ghost" onClick={cancel} disabled={sending}>
            Skip this mod
          </Button>
          <Button type="submit" tone="primary" disabled={sending || !path.trim()}>
            Use this file
          </Button>
        </Actions>
      </form>
    </Dialog>
  );
}

function ChooseVariant({ p, answer, cancel, sending }: Props<"choose_variant">) {
  return (
    <Dialog title="Choose a variant" subtitle="The archive holds several versions of the mod." onBack={cancel}>
      <div className="grid gap-2">
        {p.choices.map((c, i) => (
          <Button key={i} data-autofocus={i === 0 || undefined} disabled={sending} className="justify-start" onClick={() => answer({ type: "choice", index: i })}>
            {c}
          </Button>
        ))}
      </div>
      <Actions>
        <Button tone="ghost" onClick={cancel} disabled={sending}>
          Cancel
        </Button>
      </Actions>
    </Dialog>
  );
}

const SINGLE: GroupType[] = ["SelectExactlyOne", "SelectAtMostOne"];

/** Initial selection: the defaults plus every Required option. */
export function fomodInitial(options: FomodOption[], defaults: number[]): number[] {
  const s = new Set(defaults);
  options.forEach((o, i) => o.kind === "Required" && s.add(i));
  return [...s].sort((a, b) => a - b);
}

/** Client-side check matching the group type; null = OK. The backend re-checks. */
export function fomodProblem(kind: GroupType, n: number, total: number): string | null {
  switch (kind) {
    case "SelectExactlyOne":
      return n === 1 ? null : "Pick exactly one option.";
    case "SelectAtMostOne":
      return n <= 1 ? null : "Pick at most one option.";
    case "SelectAtLeastOne":
      return n >= 1 ? null : "Pick at least one option.";
    case "SelectAll":
      return n === total ? null : "All options are required.";
    case "SelectAny":
      return null;
  }
}

function FomodGroup({ p, answer, cancel, sending }: Props<"fomod_group">) {
  const single = SINGLE.includes(p.kind);
  const [sel, setSel] = useState(() => fomodInitial(p.options, p.defaults));
  const [focus, setFocus] = useState(sel[0] ?? 0);
  const locked = (o: FomodOption) => o.kind === "Required" || p.kind === "SelectAll";
  const toggle = (i: number) => {
    setFocus(i);
    const o = p.options[i];
    if (o.kind === "NotUsable" || locked(o)) return;
    setSel((s) => {
      if (single) return s.includes(i) && p.kind === "SelectAtMostOne" ? [] : [i];
      return s.includes(i) ? s.filter((x) => x !== i) : [...s, i].sort((a, b) => a - b);
    });
  };
  const problem = fomodProblem(p.kind, sel.length, p.options.length);
  const shown = p.options[focus];

  return (
    <Dialog
      wide
      title={`FOMOD installer: ${p.module}`}
      subtitle={
        <span className="flex flex-wrap items-center gap-2">
          <span className="text-zinc-300">{p.step}</span>
          <span aria-hidden className="text-dim">›</span>
          <span>{p.group}</span>
          <Badge>{p.kind_label}</Badge>
        </span>
      }
      onBack={cancel}
    >
      {p.error && (
        <p
          role="alert"
          className="mb-5 flex items-start gap-2 rounded-card border border-rose-400/15 bg-rose-500/5 px-4 py-3 text-body text-rose-200"
        >
          <WarningIcon aria-hidden size={16} weight="bold" className="mt-0.5 shrink-0" />
          {p.error}
        </p>
      )}
      <div className="grid gap-5 md:grid-cols-[1fr_1fr]">
        <div role={single ? "radiogroup" : "group"} aria-label={p.group} className="grid content-start gap-2">
          {p.options.map((o, i) => (
            <Choice
              key={i}
              multi={!single}
              selected={sel.includes(i)}
              disabled={o.kind === "NotUsable"}
              autoFocus={i === (sel[0] ?? 0)}
              onFocus={() => setFocus(i)}
              onClick={() => toggle(i)}
            >
              <span className="flex flex-wrap items-center gap-2 text-zinc-100">
                {o.name}
                {o.kind !== "Optional" && <Badge tone={o.kind === "NotUsable" ? "rose" : "accent"}>{o.kind_label ?? o.kind}</Badge>}
              </span>
            </Choice>
          ))}
        </div>
        <div className={cx(TILE, "p-5 text-zinc-300")} aria-live="polite">
          <div className="eyebrow mb-3">Description</div>
          <h3 className="mb-2 text-lead font-medium text-zinc-100">{shown?.name}</h3>
          <p className="text-body leading-relaxed whitespace-pre-wrap text-zinc-400">{shown?.description || "No description."}</p>
        </div>
      </div>
      {problem && <p className="mt-4 text-body text-amber-300">{problem}</p>}
      <Actions>
        <Button tone="ghost" onClick={cancel} disabled={sending}>
          Cancel install
        </Button>
        <Button tone="primary" disabled={sending || !!problem} onClick={() => answer({ type: "choices", indices: sel })}>
          Next
        </Button>
      </Actions>
    </Dialog>
  );
}
