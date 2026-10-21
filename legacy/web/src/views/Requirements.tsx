import { useCallback, useEffect, useState } from 'react'
import { motion } from 'framer-motion'
import { ArrowClockwiseIcon, ArrowSquareOutIcon, CheckCircleIcon, PuzzlePieceIcon, SealCheckIcon, WarningIcon, XCircleIcon } from '@phosphor-icons/react'
import { api, type DepStatus, type Requirement } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { cx } from '../lib/format'
import { Button, Chip, EmptyState, ErrorNote, PageHeader, Skeleton, quick } from '../components/ui'

const GROUPS: { status: DepStatus[]; title: string; blurb: string }[] = [
  { status: ['missing'], title: 'Missing', blurb: 'The mods that need these will not work until they are installed.' },
  { status: ['disabled'], title: 'Installed but disabled', blurb: 'Enable them and deploy again.' },
  { status: ['external'], title: 'External — check yourself', blurb: 'Hosted outside Nexus, so modman cannot tell whether they are installed. Mark them once you have.' },
  { status: ['ok'], title: 'Present', blurb: '' },
  { status: ['gone'], title: 'No longer on Nexus', blurb: 'Listed as required, but the page was removed. Usually safe to ignore.' },
]

export function Requirements() {
  const { run, busy, jobVersion } = useApp()
  const game = useCurrentGame()
  const [deps, setDeps] = useState<Requirement[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)

  const load = useCallback(async (refresh = false) => {
    if (!game) return
    setRefreshing(refresh)
    try {
      setDeps(await api.deps(game.domain, refresh))
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setRefreshing(false)
    }
  }, [game?.domain])

  useEffect(() => { setDeps(null); void load() }, [load, jobVersion])

  if (!game) return null
  const visibleGroups = GROUPS.map((g) => ({ ...g, items: (deps ?? []).filter((d) => g.status.includes(d.status) && !(d.status === 'external' && d.acked)) }))
  // acknowledged externals count as present
  const acked = (deps ?? []).filter((d) => d.status === 'external' && d.acked)
  visibleGroups[3].items = [...visibleGroups[3].items, ...acked]

  return (
    <div>
      <PageHeader
        eyebrow={game.name}
        title="Requirements"
        actions={<Button size="sm" disabled={refreshing} icon={<ArrowClockwiseIcon size={15} className={refreshing ? 'animate-spin' : ''} />} onClick={() => load(true)}>Re-check Nexus</Button>}
      >
        What your enabled mods need, from their Nexus pages. Loaders like UE4SS or SKSE are detected by their files in the game folder.
      </PageHeader>

      {error && <ErrorNote>Could not check requirements: {error}</ErrorNote>}

      {deps === null && !error ? (
        <div className="grid gap-3">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-24 w-full rounded-2xl" />)}</div>
      ) : deps && deps.length === 0 ? (
        <EmptyState icon={<PuzzlePieceIcon size={28} weight="duotone" />} title="No requirements listed">
          None of the enabled mods list requirements on Nexus.
        </EmptyState>
      ) : (
        <div className="grid gap-10">
          {visibleGroups.filter((g) => g.items.length).map((g) => (
            <section key={g.title}>
              <div className="mb-4 flex items-baseline gap-3 border-b border-white/[0.05] pb-3">
                <h2 className="text-[17px] font-medium tracking-tight text-zinc-100">{g.title}</h2>
                <span className="font-mono text-[12px] text-zinc-500">{g.items.length}</span>
                {g.blurb && <p className="ml-auto hidden max-w-[52ch] text-right text-[13px] text-zinc-500 md:block">{g.blurb}</p>}
              </div>
              <ul className={cx('grid gap-3', g.items.length > 1 && 'lg:grid-cols-2')}>
                {g.items.map((d, i) => (
                  <motion.li
                    key={d.key}
                    initial={{ opacity: 0, y: 4 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ ...quick, delay: Math.min(i, 10) * 0.015 }}
                  >
                    <RequirementCard dep={d} busy={busy} run={run} />
                  </motion.li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}

function RequirementCard({ dep, busy, run }: { dep: Requirement; busy: boolean; run: ReturnType<typeof useApp>['run'] }) {
  const [showAll, setShowAll] = useState(false)
  const bad = dep.status === 'missing'
  const warn = dep.status === 'disabled' || (dep.status === 'external' && !dep.acked)
  const icon = bad ? <XCircleIcon size={20} weight="duotone" className="text-rose-300" />
    : warn ? <WarningIcon size={20} weight="duotone" className="text-amber-300" />
      : dep.acked ? <SealCheckIcon size={20} weight="duotone" className="text-accent-300" />
        : <CheckCircleIcon size={20} weight="duotone" className={dep.status === 'gone' ? 'text-zinc-600' : 'text-accent-300'} />
  const [verb, target] = dep.hint.split(' ')
  const by = showAll ? dep.by : dep.by.slice(0, 3)

  return (
    <div className={cx('h-full rounded-2xl border p-5', bad ? 'border-rose-400/15 bg-rose-500/[0.035]' : 'border-white/[0.05] bg-ink-2')}>
      <div className="flex items-start gap-3">
        <span className="mt-0.5">{icon}</span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-[15.5px] font-medium text-zinc-100">{dep.name}</h3>
            {dep.key.startsWith('tool:') && <Chip>loader</Chip>}
            {dep.acked && <Chip tone="accent">marked installed</Chip>}
          </div>
          {dep.detail && <p className="mt-1 text-[13px] text-zinc-500">{dep.detail}</p>}
          {dep.by.length > 0 && (
            <p className="mt-2 text-[13px] leading-relaxed text-zinc-400">
              Needed by {by.join(', ')}
              {dep.by.length > 3 && !showAll && (
                <button onClick={() => setShowAll(true)} className="ml-1 text-zinc-300 underline decoration-white/20 underline-offset-2 hover:decoration-white/60">+{dep.by.length - 3} more</button>
              )}
            </p>
          )}
          {dep.status !== 'ok' && dep.status !== 'gone' && (
            <div className="mt-4 flex flex-wrap gap-2">
              {verb === 'install' && (
                <Button size="sm" variant="primary" disabled={busy} onClick={() => run(['install', target], { label: `Install ${dep.name} (Nexus ${target})` })}>
                  Install from Nexus
                </Button>
              )}
              {verb === 'enable' && (
                <Button size="sm" variant="primary" disabled={busy} onClick={() => run(['enable', target], { label: `Enable ${dep.name}` })}>Enable</Button>
              )}
              {dep.urls.map((u) => (
                <a key={u} href={u} target="_blank" rel="noreferrer" className="inline-flex h-9 max-w-full items-center gap-1.5 rounded-xl bg-white/[0.06] px-3 text-[13px] text-zinc-200 hover:bg-white/[0.1]">
                  <ArrowSquareOutIcon size={14} className="shrink-0" /><span className="truncate">{u.replace(/^https?:\/\/(www\.)?/, '')}</span>
                </a>
              ))}
              {(dep.status === 'external' || dep.status === 'missing') && !dep.acked && (
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(['deps', 'ack', dep.key], { label: `Mark ${dep.name} as installed` })}>
                  I installed it
                </Button>
              )}
            </div>
          )}
          {dep.acked && (
            <Button size="sm" variant="ghost" className="mt-3 -ml-3" disabled={busy} onClick={() => run(['deps', 'unack', dep.key], { label: `Check ${dep.name} again` })}>
              Check again
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
