// SPDX-License-Identifier: GPL-3.0-or-later
// Render-error containment: a view that throws shows an error note with
// "Try again" (re-render it) and "Reload" instead of unmounting the whole
// window. `ViewBoundary` wraps each view; `RootBoundary` is the last resort
// around the app.

import { WarningIcon } from "@phosphor-icons/react";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { errMsg } from "../app/bridge";
import { Button, ErrorNote } from "./ui";

interface Props {
  children: ReactNode;
  fallback: (error: unknown, retry: () => void) => ReactNode;
}

class Boundary extends Component<Props, { error: unknown; failed: boolean }> {
  state = { error: null as unknown, failed: false };

  static getDerivedStateFromError(error: unknown) {
    return { error, failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("render error", error, info.componentStack);
  }

  retry = () => this.setState({ error: null, failed: false });

  render() {
    return this.state.failed ? this.props.fallback(this.state.error, this.retry) : this.props.children;
  }
}

const reload = () => window.location.reload();

/** Per view: the sidebar and the other views keep working. */
export function ViewBoundary({ name, children }: { name: string; children: ReactNode }) {
  return (
    <Boundary
      fallback={(error, retry) => (
        <div className="grid gap-4 py-6">
          <h1 className="text-3xl font-medium tracking-tighter text-zinc-50">{name}</h1>
          <ErrorNote
            actions={
              <>
                <Button size="sm" tone="primary" data-autofocus onClick={retry}>
                  Try again
                </Button>
                <Button size="sm" onClick={reload}>
                  Reload
                </Button>
              </>
            }
          >
            This view hit an error and could not be shown: {errMsg(error)}
          </ErrorNote>
        </div>
      )}
    >
      {children}
    </Boundary>
  );
}

/** Around the whole app: never a blank window. */
export function RootBoundary({ children }: { children: ReactNode }) {
  return (
    <Boundary
      fallback={(error, retry) => (
        <div role="alert" className="grid h-full place-items-center p-10">
          <div className="grid max-w-xl gap-4">
            <WarningIcon aria-hidden size={32} weight="duotone" className="text-rose-300" />
            <h1 className="text-2xl font-medium tracking-tight text-zinc-50">modman hit an error</h1>
            <p className="text-[14px] leading-relaxed wrap-anywhere text-zinc-400">{errMsg(error)}</p>
            <div className="flex flex-wrap gap-3">
              <Button tone="primary" onClick={reload}>
                Reload
              </Button>
              <Button onClick={retry}>Try again</Button>
            </div>
          </div>
        </div>
      )}
    >
      {children}
    </Boundary>
  );
}
