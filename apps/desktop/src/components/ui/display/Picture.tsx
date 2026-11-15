// SPDX-License-Identifier: GPL-3.0-or-later
import { useQuery } from "@tanstack/react-query";
import { picture } from "../../../ipc/commands";
import { monogram } from "../format";
import { cx } from "../styles";

/**
 * A Nexus picture by URL: a data: URL from the backend cache (no remote
 * images under the CSP), else a monogram of `name`. Size and radius come
 * from `className`.
 */
export function Picture({ url, name, className }: { url: string | null; name: string; className?: string }) {
  const q = useQuery({
    queryKey: ["picture", url],
    queryFn: () => picture(url!),
    enabled: !!url,
    staleTime: Infinity,
  });
  const src = q.data && q.data.startsWith("data:image/") ? q.data : null;
  if (src) return <img src={src} alt="" aria-hidden data-testid="picture" className={cx("object-cover", className)} />;
  const m = monogram(name);
  return (
    <span
      aria-hidden
      data-testid="picture-monogram"
      className={cx("grid place-items-center text-2xl font-semibold tracking-tight text-white/85", m.cls, className)}
    >
      {m.initials}
    </span>
  );
}
