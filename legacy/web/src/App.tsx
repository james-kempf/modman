import { motion } from 'framer-motion'
import { PlugsIcon } from '@phosphor-icons/react'
import { useApp } from './lib/store'
import { Sidebar } from './components/Sidebar'
import { JobIsland } from './components/JobIsland'
import { Button, EmptyState, Skeleton, quick } from './components/ui'
import { Overview } from './views/Overview'
import { Mods } from './views/Mods'
import { Collections } from './views/Collections'
import { Requirements } from './views/Requirements'
import { Install } from './views/Install'
import { Activity } from './views/Activity'
import { Settings, Account, AddGame } from './views/Settings'

const VIEWS = { overview: Overview, mods: Mods, collections: Collections, requirements: Requirements, install: Install, activity: Activity, settings: Settings }

export default function App() {
  const { state, loading, error, view, refresh } = useApp()
  const noGames = state && state.games.length === 0
  const View = VIEWS[view]

  return (
    <div className="flex min-h-[100dvh] flex-col lg:flex-row">
      <div className="grain" aria-hidden />
      {!noGames && <Sidebar />}
      <main className="min-w-0 flex-1">
        <div className="mx-auto w-full max-w-[1400px] px-4 pt-8 pb-32 md:px-10 md:pt-12">
          {loading ? (
            <LoadingShell />
          ) : !state ? (
            <EmptyState
              icon={<PlugsIcon size={28} weight="duotone" />}
              title="Lost contact with modman"
              action={<Button variant="primary" onClick={() => refresh()}>Try again</Button>}
            >
              {error ?? 'The interface could not reach its server.'} If you closed the terminal running <span className="font-mono text-zinc-300">modman ui</span>, start it again.
            </EmptyState>
          ) : noGames ? (
            <Onboarding />
          ) : (
            <motion.div key={`${view}-${state.selectedGame}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={quick}>
              <View />
            </motion.div>
          )}
        </div>
      </main>
      <JobIsland />
    </div>
  )
}

function LoadingShell() {
  return (
    <div className="grid gap-6">
      <div className="grid gap-6 xl:grid-cols-[1.35fr_1fr]">
        <Skeleton className="h-80 rounded-[2rem]" />
        <Skeleton className="h-80 rounded-[2rem]" />
      </div>
      <div className="grid gap-6 md:grid-cols-[2fr_1fr]">
        <Skeleton className="h-56 rounded-3xl" />
        <Skeleton className="h-56 rounded-3xl" />
      </div>
    </div>
  )
}

function Onboarding() {
  const { state } = useApp()
  return (
    <div className="grid gap-12 lg:grid-cols-[0.9fr_1.1fr] lg:gap-16">
      <div className="lg:pt-10">
        <div className="eyebrow">First run</div>
        <h1 className="mt-4 text-4xl leading-none font-medium tracking-tighter text-zinc-50 md:text-6xl">Set up modman</h1>
        <p className="mt-5 max-w-[48ch] text-[16px] leading-relaxed text-zinc-400">
          Sign in with your Nexus Mods API key, then pick a game. modman keeps every mod in its own folder and links
          them into the game only when you deploy, so the game folder can always go back to how it was.
        </p>
        <ol className="mt-10 grid gap-5 border-l border-white/[0.07] pl-6">
          {['Sign in to Nexus Mods', 'Add a game', 'Install a mod or a collection, then deploy'].map((s, i) => (
            <li key={s} className="text-[14.5px] text-zinc-300"><span className="mr-3 font-mono text-[12px] text-zinc-500">0{i + 1}</span>{s}</li>
          ))}
        </ol>
      </div>
      <div className="grid gap-12">
        {!state?.account.loggedIn && <Account />}
        <AddGame />
      </div>
    </div>
  )
}
