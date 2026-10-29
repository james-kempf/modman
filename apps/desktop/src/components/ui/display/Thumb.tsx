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
  if (src) return <img src={src} alt="" className={cx("size-thumb shrink-0 rounded-control object-cover", className)} />;
  const m = monogram(name ?? "");
  return (
    <span
      aria-hidden
      className={cx(
        "grid size-thumb shrink-0 place-items-center rounded-control font-mono text-micro font-semibold text-white/75 in-highlight:text-white/75",
        m.cls,
        className,
      )}
    >
      {name ? m.initials : ""}
    </span>
  );
}
