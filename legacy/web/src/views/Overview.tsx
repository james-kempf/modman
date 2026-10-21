import { useEffect, useState } from 'react'
import { motion } from 'framer-motion'
import {
  ArrowRightIcon, ArrowsClockwiseIcon, BroomIcon, CopyIcon, PlayIcon, PuzzlePieceIcon, RocketLaunchIcon, ShieldCheckIcon, WarningIcon,
} from '@phosphor-icons/react'
import { api, type Requirement } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { ago, bytes, cx, plural } from '../lib/format'
import { Button, Chip, ConfirmDialog, CtaButton, Skeleton, quick, snappy, StatusDot, Thumb } from '../components/ui'

const rise = {
  hidden: { opacity: 0, y: 4 },
  show: (i: number) => ({ opacity: 1, y: 0, transition: { ...quick, delay: i * 0.015 } }),
}

export function Overview() {
  const { state, run, busy, go, thumbs, jobVersion } = useApp()
  const game = useCurrentGame()
  const [confirmPurge, setConfirmPurge] = useState(false)
  const [deps, setDeps] = useState<Requirement[] | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!game) return
    let alive = true
    setDeps(null)
    api.deps(game.domain).then((d) => alive && setDeps(d)).catch(() => alive && setDeps([]))
    return () => { alive = false }
  }, [game?.domain, jobVersion])

  if (!state || !game) return null
  const problems = (deps ?? []).filter((d) => d.status === 'missing' || d.status === 'disabled' || (d.status === 'external' && !d.acked))
  const enabledPct = game.mods ? Math.round((game.enabled / game.mods) * 1000) / 10 : 0
  const recent = [...state.mods].sort((a, b) => (b.installed ?? 0) - (a.installed ?? 0)).slice(0, 5)

  const status = !game.folderExists
    ? { tone: 'rose' as const, text: 'Game folder not found' }
    : game.deployed
      ? game.changedSinceDeploy
        ? { tone: 'amber' as const, text: 'Changes waiting to be deployed' }
        : { tone: 'accent' as const, text: 'Deployed and in sync' }
      : { tone: 'zinc' as const, text: 'Not deployed' }

  return (
    <div className="grid gap-6">
      {/* Hero — identity left, controls right */}
      <motion.section initial="hidden" animate="show" className="grid gap-6 xl:grid-cols-[1.35fr_1fr]">
        <motion.div custom={0} variants={rise} className="relative overflow-hidden rounded-[2rem] border border-white/[0.06] bg-ink-2 p-7 md:p-10">
          <div className="pointer-events-none absolute -top-24 -right-24 size-80 rounded-full bg-accent-500/[0.07] blur-3xl" />
          <div className="relative">
            <div className="flex flex-wrap items-center gap-2.5">
              <StatusDot tone={status.tone} live={game.deployed} />
              <span className="text-[13.5px] text-zinc-300">{status.text}</span>
              <Chip>{game.profile}</Chip>
              {game.appid && <Chip>steam {game.appid}</Chip>}
            </div>
            <h1 className="mt-6 text-4xl leading-none font-medium tracking-tighter text-zinc-50 md:text-6xl">{game.name}</h1>
            <button
              onClick={() => { navigator.clipboard?.writeText(game.folder); setCopied(true); setTimeout(() => setCopied(false), 1400) }}
              className="group mt-4 flex max-w-full items-center gap-2 text-left font-mono text-[12.5px] text-zinc-500 hover:text-zinc-300"
              title="Copy path"
            >
              <span className="truncate">{game.folder || 'no folder set'}</span>
              <CopyIcon size={13} className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100" />
              {copied && <span className="text-accent-300">copied</span>}
            </button>

            <dl className="mt-10 grid grid-cols-2 gap-x-8 gap-y-6 md:grid-cols-4">
              <Stat label="Enabled" value={`${game.enabled}`} sub={`of ${game.mods} mods`} />
              <Stat label="Deployed" value={game.files.toLocaleString()} sub={game.deployedAt ? ago(game.deployedAt) : 'never'} />
              <Stat label="Staged" value={bytes(game.size)} sub={`${game.collections} ${game.collections === 1 ? 'collection' : 'collections'}`} />
              <Stat label="Method" value={game.method || state.config.deployMethod} sub="deploy" />
            </dl>
          </div>
        </motion.div>

        <motion.div custom={1} variants={rise} className="flex flex-col justify-between gap-8 rounded-[2rem] border border-white/[0.06] bg-ink-2 p-7 md:p-9">
          <div>
            <div className="eyebrow">Deployment</div>
            <p className="mt-3 max-w-[40ch] text-[15px] leading-relaxed text-zinc-400">
              {game.changedSinceDeploy
                ? 'You changed mods since the last deploy. Deploy to apply them to the game folder.'
                : game.deployed
                  ? 'Your enabled mods are linked into the game. Purge restores the original files.'
                  : 'Link every enabled mod into the game folder. Original files are backed up first.'}
            </p>
          </div>
          <div className="grid gap-3">
            <CtaButton
              disabled={busy || !game.folderExists || game.enabled === 0}
              onClick={() => run(['deploy'], { label: `Deploy ${game.name}` })}
              icon={<RocketLaunchIcon size={19} weight="duotone" />}
              className="w-full"
            >
              {game.deployed ? 'Redeploy' : 'Deploy'} {plural(game.enabled, 'mod')}
            </CtaButton>
            <div className="grid grid-cols-3 gap-2">
              <Button disabled={busy || !game.deployed} onClick={() => run(['verify'], { label: 'Verify deployment' })} icon={<ShieldCheckIcon size={16} />}>Verify</Button>
              <Button disabled={busy || !game.deployed} variant="danger" onClick={() => setConfirmPurge(true)} icon={<BroomIcon size={16} />}>Purge</Button>
              <Button disabled={!game.appid} onClick={() => run(['launch'], { label: `Launch ${game.name}` })} icon={<PlayIcon size={16} weight="fill" />}>Play</Button>
            </div>
          </div>
        </motion.div>
      </motion.section>

      {/* Bento — asymmetric tiles */}
      <motion.section initial="hidden" animate="show" className="grid gap-6 md:grid-cols-6 lg:grid-cols-12">
        <motion.button
          custom={2} variants={rise}
          onClick={() => go('requirements')}
          className="group surface flex flex-col p-7 text-left md:col-span-3 lg:col-span-5"
        >
          <div className="flex items-center justify-between">
            <span className="eyebrow">Requirements</span>
            <PuzzlePieceIcon size={18} className="text-zinc-600" />
          </div>
          {deps === null ? (
            <div className="mt-6 grid gap-3"><Skeleton className="h-9 w-24" /><Skeleton className="h-4 w-full" /><Skeleton className="h-4 w-3/4" /></div>
          ) : problems.length ? (
            <>
              <div className="mt-5 flex items-baseline gap-2">
                <span className="font-mono text-4xl font-medium tracking-tight text-amber-300">{problems.length}</span>
                <span className="text-[14px] text-zinc-400">need attention</span>
              </div>
              <ul className="mt-5 grid gap-2.5">
                {problems.slice(0, 3).map((p) => (
                  <li key={p.key} className="flex items-center gap-2.5 text-[14px] text-zinc-300">
                    <WarningIcon size={14} className={p.status === 'missing' ? 'text-rose-300' : 'text-amber-300'} />
                    <span className="truncate">{p.name}</span>
                    <span className="ml-auto font-mono text-[11px] text-zinc-500">{p.by.length} mods</span>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <div className="mt-5">
              <span className="font-mono text-4xl font-medium tracking-tight text-accent-300">{deps.filter((d) => d.status === 'ok').length}</span>
              <p className="mt-2 text-[14px] text-zinc-400">requirements present. Nothing missing.</p>
            </div>
          )}
          <span className="mt-auto flex items-center gap-1.5 pt-6 text-[13px] text-zinc-500 transition-colors group-hover:text-zinc-300">
            Open requirements <ArrowRightIcon size={13} />
          </span>
        </motion.button>

        <motion.div custom={3} variants={rise} className="surface p-7 md:col-span-3 lg:col-span-4">
          <div className="flex items-center justify-between">
            <span className="eyebrow">Auto-update</span>
            <ArrowsClockwiseIcon size={18} className="text-zinc-600" />
          </div>
          <div className="mt-5 flex items-center gap-3">
            <StatusDot tone={state.autoupdate.scheduler ? (game.autoupdate.failed ? 'rose' : 'accent') : 'zinc'} live={!!state.autoupdate.scheduler} />
            <span className="text-[17px] font-medium text-zinc-100">{state.autoupdate.scheduler ? 'On' : 'Off'}</span>
            {state.autoupdate.scheduler && <Chip>{state.autoupdate.scheduler}</Chip>}
          </div>
          <p className="mt-3 text-[14px] leading-relaxed text-zinc-400">
            {state.autoupdate.scheduler
              ? <>Updates mods and redeploys after system or Steam updates. Last run {ago(game.autoupdate.lastOk)}{game.autoupdate.failed && <span className="text-rose-300"> (failed)</span>}.</>
              : 'Turn it on to keep mods updated and redeployed after system or game updates.'}
          </p>
          <div className="mt-6 flex gap-2">
            {state.autoupdate.scheduler ? (
              <Button size="sm" disabled={busy} onClick={() => run(['autoupdate', 'run', '--force'], { label: 'Run auto-update now', game: null })}>Run now</Button>
            ) : (
              <Button size="sm" variant="primary" disabled={busy} onClick={() => run(['autoupdate', 'enable'], { label: 'Enable auto-update', game: null })}>Turn on</Button>
            )}
            <Button size="sm" variant="ghost" onClick={() => go('settings')}>Settings</Button>
          </div>
        </motion.div>

        <motion.div custom={4} variants={rise} className="surface p-7 md:col-span-6 lg:col-span-3">
          <span className="eyebrow">Enabled</span>
          <div className="mt-5 flex items-baseline gap-2">
            <span className="font-mono text-4xl font-medium tracking-tight text-zinc-50">{enabledPct}</span>
            <span className="text-[14px] text-zinc-400">% of mods enabled</span>
          </div>
          <div className="mt-5 h-2 overflow-hidden rounded-full bg-white/[0.05]">
            <motion.div
              className="h-full origin-left rounded-full bg-accent-400"
              initial={{ scaleX: 0 }}
              animate={{ scaleX: enabledPct / 100 }}
              transition={{ ...snappy, delay: 0.05 }}
            />
          </div>
          <p className="mt-4 text-[14px] text-zinc-400">
            {game.mods - game.enabled > 0 ? `${game.mods - game.enabled} disabled mods stay staged but are not deployed.` : 'Every installed mod is enabled.'}
          </p>
        </motion.div>

        <motion.div custom={5} variants={rise} className="surface p-7 md:col-span-6 lg:col-span-8">
          <div className="flex items-center justify-between">
            <span className="eyebrow">Recently installed</span>
            <button onClick={() => go('mods')} className="flex items-center gap-1.5 text-[13px] text-zinc-500 hover:text-zinc-300">All mods <ArrowRightIcon size={13} /></button>
          </div>
          {recent.length ? (
            <ul className="mt-4 divide-y divide-white/[0.05]">
              {recent.map((m) => (
                <li key={m.id} className="flex items-center gap-4 py-3">
                  <Thumb src={m.modId ? thumbs[String(m.modId)] : undefined} name={m.name} className="size-10 shrink-0 rounded-xl" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[14.5px] text-zinc-100">{m.name}</div>
                    <div className="truncate font-mono text-[11.5px] text-zinc-500">{m.version || '—'} · {m.author || 'unknown author'}</div>
                  </div>
                  <span className="hidden font-mono text-[11.5px] text-zinc-500 sm:block">{ago(m.installed)}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-4 text-[14px] text-zinc-500">No mods yet. <button onClick={() => go('install')} className="text-accent-300 hover:text-accent-200">Get some</button>.</p>
          )}
        </motion.div>

        <motion.div custom={6} variants={rise} className="surface flex flex-col p-7 md:col-span-6 lg:col-span-4">
          <span className="eyebrow">Collections</span>
          {state.collections.length ? (
            <ul className="mt-4 grid gap-4">
              {state.collections.map((c) => (
                <li key={c.slug}>
                  <div className="truncate text-[14.5px] text-zinc-100">{c.name}</div>
                  <div className="mt-1 font-mono text-[11.5px] text-zinc-500">rev {c.revision} · {c.enabledMods}/{c.installedMods} enabled{c.failed.length ? ` · ${c.failed.length} failed` : ''}</div>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-4 text-[14px] leading-relaxed text-zinc-500">No collections. Install one from a Nexus collection URL.</p>
          )}
          <Button size="sm" variant="ghost" className={cx('mt-auto self-start', state.collections.length ? 'translate-y-2' : '')} onClick={() => go('collections')}>
            Manage <ArrowRightIcon size={13} />
          </Button>
        </motion.div>
      </motion.section>

      <ConfirmDialog
        open={confirmPurge}
        title={`Purge ${game.name}?`}
        body={<>Removes all {game.files.toLocaleString()} deployed files and restores the original game files. Your installed mods stay staged and can be deployed again.</>}
        confirm="Purge"
        onConfirm={() => run(['purge'], { label: `Purge ${game.name}` })}
        onClose={() => setConfirmPurge(false)}
      />
    </div>
  )
}

function Stat({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-2 truncate font-mono text-2xl font-medium tracking-tight text-zinc-50">{value}</dd>
      <dd className="mt-1 truncate text-[12.5px] text-zinc-500">{sub}</dd>
    </div>
  )
}
