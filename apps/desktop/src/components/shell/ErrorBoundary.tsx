// SPDX-License-Identifier: GPL-3.0-or-later
// Render-error containment: a view that throws shows an error note with
// "Try again" and "Reload" instead of unmounting the whole window.
// `ViewBoundary` wraps each view; `RootBoundary` is the last resort around
// the app. Render errors usually come from bad backend data, which the query
// cache would keep serving: a view's recovery drops the cached data of the
// queries nobody observes (the crashed view's) and refetches the observed
// ones before rendering again.

import { WarningIcon } from "@phosphor-icons/react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { errMsg } from "../../app/bridge";
import { focusables } from "../../nav/spatial";
import { Button, ErrorNote } from "../ui";

interface Props {
  children: ReactNode;
  fallback: (error: unknown, retry: () => Promise<void>) => ReactNode;
  /** Fresh data for a retry (awaited before rendering again). */
  recover?: () => Promise<unknown>;
}

class Boundary extends Component<Props, { error: unknown; failed: boolean; autoTried: boolean }> {
  state = { error: null as unknown, failed: false, autoTried: false };
  private unmounted = false;

  static getDerivedStateFromError(error: unknown) {
    return { error, failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("render error", error, info.componentStack);
    const recover = this.props.recover;
    if (!recover || this.state.autoTried) return;
    // Stale data in the cache (e.g. back on a view after the backend
    // recovered): refresh and retry once by itself; the error shows only
    // when that fails too.
    void recover()
      .catch(() => undefined)
      .then(() => !this.unmounted && this.setState({ error: null, failed: false, autoTried: true }));
  }

  // StrictMode unmounts and remounts the same instance: track both.
  componentDidMount() {
    this.unmounted = false;
  }

  componentWillUnmount() {
    this.unmounted = true;
  }

  retry = async () => {
    await this.props.recover?.().catch(() => undefined);
    if (!this.unmounted) this.setState({ error: null, failed: false });
  };

  render() {
    if (!this.state.failed) return this.props.children;
    if (this.props.recover && !this.state.autoTried) return <p className="py-6 text-body text-zinc-400">Loading…</p>;
    return this.props.fallback(this.state.error, this.retry);
  }
}

const reload = () => window.location.reload();

/** Drop cached data nobody observes (the crashed view's queries) and refetch the observed ones. */
export function recoverQueries(qc: QueryClient): Promise<unknown> {
  return Promise.all([qc.resetQueries({ type: "inactive" }), qc.refetchQueries({ type: "active" })]);
}

/** After a retry, focus left on <body> (the Try again button is gone) goes to the view's first control. */
function refocusView(): void {
  requestAnimationFrame(() => {
    const a = document.activeElement;
    if (a && a !== document.body && a.isConnected) return;
    const main = document.querySelector("main");
    (main ? focusables(main)[0] : null)?.focus();
  });
}

/** Per view: the sidebar and the other views keep working. */
export function ViewBoundary({ name, children }: { name: string; children: ReactNode }) {
  const qc = useQueryClient();
  return (
    <Boundary
      recover={() => recoverQueries(qc)}
      fallback={(error, retry) => (
        <div className="grid gap-4 py-6">
          <h1 className="text-3xl font-medium tracking-tighter text-zinc-50">{name}</h1>
          <ErrorNote
            actions={
              <>
                <Button
                  size="sm"
                  tone="primary"
                  data-autofocus
                  onClick={() => void retry().then(refocusView)}
                >
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
            <h1 className="text-2xl font-medium tracking-tight text-zinc-50">Vessel hit an error</h1>
            <p className="text-body leading-relaxed wrap-anywhere text-zinc-400">{errMsg(error)}</p>
            <div className="flex flex-wrap gap-3">
              <Button tone="primary" onClick={reload}>
                Reload
              </Button>
              <Button onClick={() => void retry()}>Try again</Button>
            </div>
          </div>
        </div>
      )}
    >
      {children}
    </Boundary>
  );
}
