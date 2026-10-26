// SPDX-License-Identifier: GPL-3.0-or-later
// TanStack Query hooks over the read-only commands.

import { QueryClient, useQuery } from "@tanstack/react-query";
import * as ipc from "../ipc/commands";

export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      // Backend data changes only through our own commands and jobs, which invalidate.
      queries: { retry: false, refetchOnWindowFocus: false, staleTime: 5_000 },
    },
  });
}

/** Picture queries: no command changes them (game art's key holds the app id) and they may need the network. */
const PICTURES = new Set(["art", "collection-art"]);

/**
 * Refresh after a command or job: every query except pictures. `refetchType: "all"`
 * also refetches queries whose view is not mounted (the Overview counts), so they
 * are current when the user switches to that view instead of only after it mounts.
 */
export const refreshAll = (qc: QueryClient) =>
  qc.invalidateQueries({ predicate: (q) => !PICTURES.has(String(q.queryKey[0])), refetchType: "all" });

export const useGames = () => useQuery({ queryKey: ["games"], queryFn: ipc.gamesList });

export function useActiveGame() {
  const q = useGames();
  return { ...q, game: q.data?.find((g) => g.active) ?? null };
}

export const useMods = (game: string | null) =>
  useQuery({ queryKey: ["mods", game], queryFn: () => ipc.modsList(game), enabled: !!game });

export const useAccount = () => useQuery({ queryKey: ["account"], queryFn: ipc.account });
