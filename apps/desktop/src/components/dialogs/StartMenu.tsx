// SPDX-License-Identifier: GPL-3.0-or-later
// The controller Start menu: gamepad Start opens it in controller mode (nav/actions.ts "menu"), B or Start
// closes it. Deploy, Verify and Purge run the active game's actions exactly as the Overview's Deployment card
// does (Purge's confirmation prompt included); Quit closes the app (the window's close button is out of reach
// in Game Mode).

import { BroomIcon, LightningIcon, PowerIcon, ShieldCheckIcon } from "@phosphor-icons/react";
import { useActiveGame } from "../../app/queries";
import { useBusy, useDeploy } from "../../app/run";
import { useUi } from "../../app/store";
import * as ipc from "../../ipc/commands";
import { START_MENU_ATTR } from "../../nav/actions";
import { usePurge, useVerify } from "../../views/Overview";
import { Button } from "../ui";
import { Dialog } from "./Dialog";

/** Rendered by the shell while `startMenu` is set in the UI store. */
export function StartMenuHost() {
  const open = useUi((s) => s.startMenu);
  return open ? <StartMenu /> : null;
}

export function StartMenu() {
  const close = () => useUi.getState().setStartMenu(false);
  const { game } = useActiveGame();
  const deploy = useDeploy();
  const verify = useVerify();
  const purge = usePurge();
  const deploying = useBusy("Deploy");
  const verifying = useBusy("Verify");
  const purging = useBusy("Purge");
  // Close first: the action's own prompts (Purge's confirmation) open over the app, and its toast is in view.
  const then = (fn: () => Promise<unknown>) => () => {
    close();
    void fn();
  };
  const noGame = !game;
  return (
    <Dialog title="Menu" subtitle={noGame ? "Pick a game in Games to deploy, verify or purge it." : game.name} onBack={close}>
      <div {...{ [START_MENU_ATTR]: "" }} className="grid gap-2">
        <Button
          tone="primary"
          size="lg"
          busy={deploying}
          inactive={noGame}
          icon={<LightningIcon aria-hidden size={19} weight="duotone" />}
          onClick={then(() => deploy())}
        >
          Deploy
        </Button>
        <Button size="lg" busy={verifying} inactive={noGame} icon={<ShieldCheckIcon aria-hidden size={17} />} onClick={then(verify)}>
          Verify
        </Button>
        <Button tone="danger" size="lg" busy={purging} inactive={noGame} icon={<BroomIcon aria-hidden size={17} />} onClick={then(purge)}>
          Purge
        </Button>
        <Button tone="ghost" size="lg" icon={<PowerIcon aria-hidden size={17} />} onClick={() => void ipc.quitApp()}>
          Quit
        </Button>
      </div>
    </Dialog>
  );
}
