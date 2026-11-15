// SPDX-License-Identifier: GPL-3.0-or-later
import { Button, cx } from "../../components/ui";

export type BulkAction = "enable" | "disable" | "uninstall";

/**
 * Actions on the selected load-order rows, stuck to the bottom of the list
 * (the Browse pagination toolbar's sticky pattern). Renders nothing at 0.
 * While an action runs (`busy`), it spins and every button is inactive
 * (aria-disabled, still focusable). Hiding the bar is the caller's cue to put
 * focus back on a row; B/Escape does not clear (B is Back).
 */
export function BulkBar({
  count,
  busy = null,
  onEnable,
  onDisable,
  onUninstall,
  onClear,
  className,
}: {
  count: number;
  busy?: BulkAction | null;
  onEnable: () => void;
  onDisable: () => void;
  onUninstall: () => void;
  onClear: () => void;
  className?: string;
}) {
  if (count <= 0) return null;
  const state = (a: BulkAction) => ({ busy: busy === a, inactive: busy !== null && busy !== a });
  return (
    <div
      role="region"
      aria-label="Selected mods"
      className={cx(
        // bottom: see the Browse pager (WebKit insets sticky by <main>'s padding-bottom).
        "sticky bottom-[calc(var(--main-pb,0px)*-1)] z-10 flex flex-wrap items-center gap-2 border-t border-hairline bg-ink-2 px-section py-3",
        className,
      )}
    >
      <span className="mr-auto text-label text-zinc-300 tabular-nums">{count} selected</span>
      <Button size="sm" {...state("enable")} onClick={onEnable}>
        Enable
      </Button>
      <Button size="sm" {...state("disable")} onClick={onDisable}>
        Disable
      </Button>
      <Button size="sm" tone="danger" {...state("uninstall")} onClick={onUninstall}>
        Uninstall
      </Button>
      <Button size="sm" tone="ghost" inactive={busy !== null} onClick={onClear}>
        Clear
      </Button>
    </div>
  );
}
