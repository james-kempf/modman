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

/**
 * Refresh after a command or job: every query except game art, which no
 * command changes (its key holds the app id) and which may need the network.
 */
export const refreshAll = (qc: QueryClient) => qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "art" });

export const useGames = () => useQuery({ queryKey: ["games"], queryFn: ipc.gamesList });

export function useActiveGame() {
  const q = useGames();
  return { ...q, game: q.data?.find((g) => g.active) ?? null };
}

export const useMods = (game: string | null) =>
  useQuery({ queryKey: ["mods", game], queryFn: () => ipc.modsList(game), enabled: !!game });

export const useAccount = () => useQuery({ queryKey: ["account"], queryFn: ipc.account });
