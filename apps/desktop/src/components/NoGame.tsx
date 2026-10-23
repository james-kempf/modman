// SPDX-License-Identifier: GPL-3.0-or-later
// Empty state of the per-game views while no game is active: points back to
// the Games view, where games are picked and added.

import { GameControllerIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { openView } from "../nav/actions";
import { Button, Empty } from "./ui";

export function NoGame({ icon, title = "No active game", children }: { icon?: ReactNode; title?: string; children?: ReactNode }) {
  return (
    <Empty
      icon={icon ?? <GameControllerIcon size={28} weight="duotone" />}
      title={title}
      action={
        <Button tone="primary" icon={<GameControllerIcon aria-hidden size={16} />} onClick={() => openView("games")}>
          Go to Games
        </Button>
      }
    >
      {children ?? "Pick a game, or add one, in Games."}
    </Empty>
  );
}
