import { motion } from 'framer-motion'
import {
  BooksIcon, DownloadSimpleIcon, GearSixIcon, PuzzlePieceIcon, SquaresFourIcon, StackIcon, TerminalWindowIcon, PlusIcon,
} from '@phosphor-icons/react'
import type { ReactNode } from 'react'
import { useApp, type View } from '../lib/store'
import { cx } from '../lib/format'
import { snappy, StatusDot } from './ui'

const NAV: { view: View; label: string; icon: ReactNode }[] = [
  { view: 'overview', label: 'Overview', icon: <SquaresFourIcon size={19} weight="duotone" /> },
  { view: 'mods', label: 'Mods', icon: <StackIcon size={19} weight="duotone" /> },
  { view: 'collections', label: 'Collections', icon: <BooksIcon size={19} weight="duotone" /> },
  { view: 'requirements', label: 'Requirements', icon: <PuzzlePieceIcon size={19} weight="duotone" /> },
  { view: 'install', label: 'Get mods', icon: <DownloadSimpleIcon size={19} weight="duotone" /> },
  { view: 'activity', label: 'Activity', icon: <TerminalWindowIcon size={19} weight="duotone" /> },
  { view: 'settings', label: 'Settings', icon: <GearSixIcon size={19} weight="duotone" /> },
]

function Brand() {
  const { state } = useApp()
  return (
    <div className="flex items-center gap-3">
      <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
        <rect width="32" height="32" rx="9" className="fill-ink-3" />
        <path d="M8 22V10l8 7 8-7v12" fill="none" className="stroke-accent-400" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <div className="leading-tight">
        <div className="text-[15px] font-semibold tracking-tight text-zinc-50">modman</div>
        <div className="font-mono text-[10.5px] text-zinc-500">v{state?.version ?? '…'}</div>
      </div>
    </div>
  )
}

export function Sidebar() {
  const { state, view, go, setGame } = useApp()
  const games = state?.games ?? []
  const sel = state?.selectedGame
  const current = games.find((g) => g.domain === sel)

  return (
    <>
      {/* Desktop / Steam Deck rail */}
      <aside className="sticky top-0 hidden h-[100dvh] w-[268px] shrink-0 flex-col border-r border-white/[0.05] bg-ink lg:flex">
        <div className="px-6 pt-7 pb-6"><Brand /></div>

        <div className="px-4">
          <div className="eyebrow px-2 pb-2">Games</div>
          <div className="grid gap-0.5">
            {games.map((g) => (
              <button
                key={g.domain}
                onClick={() => setGame(g.domain)}
                className={cx(
                  'group flex h-11 items-center gap-3 rounded-xl px-2.5 text-left transition-colors duration-200',
                  g.domain === sel ? 'bg-white/[0.06] text-zinc-50' : 'text-zinc-400 hover:bg-white/[0.03] hover:text-zinc-200',
                )}
              >
                <StatusDot tone={g.deployed ? (g.changedSinceDeploy ? 'amber' : 'accent') : 'zinc'} live={g.deployed && g.domain === sel} />
                <span className="min-w-0 flex-1 truncate text-[14px]">{g.name}</span>
                <span className="font-mono text-[11px] text-zinc-600">{g.mods}</span>
              </button>
            ))}
            <button
              onClick={() => go('settings')}
              className="flex h-10 items-center gap-3 rounded-xl px-2.5 text-[13px] text-zinc-500 transition-colors hover:text-zinc-300"
            >
              <PlusIcon size={14} /> Add a game
            </button>
          </div>
        </div>

        <nav className="mt-6 px-4">
          <div className="eyebrow px-2 pb-2">{current?.name ?? 'Library'}</div>
          <ul className="grid gap-0.5">
            {NAV.map((n) => (
              <li key={n.view}>
                <button
                  onClick={() => go(n.view)}
                  className={cx(
                    'relative flex h-11 w-full items-center gap-3 rounded-xl px-3 text-[14px] transition-colors duration-200',
                    view === n.view ? 'text-zinc-50' : 'text-zinc-400 hover:text-zinc-200',
                  )}
                >
                  {view === n.view && (
                    <motion.span layoutId="nav-pill" transition={snappy} className="absolute inset-0 rounded-xl bg-white/[0.06]" />
                  )}
                  <span className={cx('relative', view === n.view && 'text-accent-300')}>{n.icon}</span>
                  <span className="relative">{n.label}</span>
                  {n.view === 'overview' && current?.changedSinceDeploy && (
                    <span className="relative ml-auto"><StatusDot tone="amber" live /></span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <div className="mt-auto border-t border-white/[0.05] px-6 py-5">
          {state?.account.loggedIn ? (
            <div className="flex items-center gap-3">
              <div className="grid size-9 place-items-center rounded-xl bg-gradient-to-br from-accent-600/50 to-ink-3 font-mono text-[12px] font-semibold text-accent-50">
                {state.account.user.slice(0, 2).toUpperCase()}
              </div>
              <div className="min-w-0 leading-tight">
                <div className="truncate text-[13.5px] text-zinc-200">{state.account.user}</div>
                <div className="font-mono text-[10.5px] text-zinc-500">{state.account.premium ? 'Premium' : 'Free account'}</div>
              </div>
            </div>
          ) : (
            <button onClick={() => go('settings')} className="text-[13px] text-accent-300 hover:text-accent-200">Sign in to Nexus Mods</button>
          )}
        </div>
      </aside>

      {/* Phone / narrow layout */}
      <header className="sticky top-0 z-30 border-b border-white/[0.05] bg-ink/90 backdrop-blur-xl lg:hidden">
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <Brand />
          {games.length > 0 && (
            <select
              value={sel ?? ''}
              onChange={(e) => setGame(e.target.value)}
              className="input h-10 max-w-[55%] text-[14px]"
              aria-label="Game"
            >
              {games.map((g) => <option key={g.domain} value={g.domain}>{g.name}</option>)}
            </select>
          )}
        </div>
        <nav className="flex gap-1 overflow-x-auto px-3 pb-3">
          {NAV.map((n) => (
            <button
              key={n.view}
              onClick={() => go(n.view)}
              className={cx(
                'flex h-9 shrink-0 items-center gap-2 rounded-xl px-3 text-[13px]',
                view === n.view ? 'bg-white/[0.07] text-zinc-50' : 'text-zinc-400',
              )}
            >
              {n.icon}{n.label}
            </button>
          ))}
        </nav>
      </header>
    </>
  )
}
