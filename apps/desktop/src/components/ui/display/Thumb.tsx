// SPDX-License-Identifier: GPL-3.0-or-later
import { useQuery } from "@tanstack/react-query";
import { thumbnail } from "../../../ipc/commands";
import { monogram } from "../format";
import { cx } from "../styles";

/** Mod picture: a data: URL from the backend cache (no remote images under the CSP), else a monogram. */
export function Thumb({
  game,
  modId,
  name,
  className,
}: {
  game: string;
  modId: number | null;
  name?: string;
  className?: string;
}) {
  const q = useQuery({
    queryKey: ["thumb", game, modId],
    queryFn: () => thumbnail(game, modId!),
    enabled: modId !== null,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:") ? q.data : null;
  // 16:9 at the row's thumb height; the monogram shares the box so loading the picture never shifts the row.
  const box = "h-thumb aspect-video shrink-0 rounded-control";
  if (src) return <img src={src} alt="" data-testid="mod-thumb" className={cx(box, "object-cover", className)} />;
  const m = monogram(name ?? "");
  return (
    <span
      aria-hidden
      data-testid="mod-thumb"
      className={cx(
        box,
        "grid place-items-center font-mono text-micro font-semibold text-white/75 in-highlight:text-white/75",
        m.cls,
        className,
      )}
    >
      {name ? m.initials : ""}
    </span>
  );
}
