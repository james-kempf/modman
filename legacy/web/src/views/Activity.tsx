import { useEffect, useState } from 'react'
import { ArrowClockwiseIcon, TerminalWindowIcon } from '@phosphor-icons/react'
import { motion } from 'framer-motion'
import { api, type JobSummary } from '../lib/api'
import { useApp, useLiveJob } from '../lib/store'
import { ago, cx } from '../lib/format'
import { Button, EmptyState, PageHeader, Skeleton, spring, StatusDot } from '../components/ui'
import { ConsoleView } from '../components/JobIsland'

export function Activity() {
  const { jobVersion } = useApp()
  const live = useLiveJob()
  const [tab, setTab] = useState<'actions' | 'log'>('actions')
  const [jobs, setJobs] = useState<JobSummary[] | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [output, setOutput] = useState<string[]>([])
  const [log, setLog] = useState<{ lines: string[]; file: string } | null>(null)

  useEffect(() => {
    api.jobs().then((r) => {
      setJobs(r.jobs)
      setSelected((s) => s ?? r.jobs[0]?.id ?? null)
    }).catch(() => setJobs([]))
  }, [jobVersion, live?.summary?.id])

  const liveSelected = live?.summary?.id === selected
  useEffect(() => {
    if (!selected || liveSelected) return
    api.job(selected).then((j) => setOutput(j.output)).catch(() => setOutput([]))
  }, [selected, liveSelected, jobVersion])

  const loadLog = () => api.log(500).then(setLog).catch(() => setLog({ lines: [], file: '' }))
  useEffect(() => { if (tab === 'log') void loadLog() }, [tab, jobVersion])

  return (
    <div>
      <PageHeader
        title="Activity"
        actions={
          <div className="flex h-11 rounded-xl border border-white/[0.06] bg-ink p-1">
            {(['actions', 'log'] as const).map((t) => (
              <button key={t} onClick={() => setTab(t)} className={cx('relative rounded-lg px-4 text-[13px]', tab === t ? 'text-zinc-50' : 'text-zinc-500 hover:text-zinc-300')}>
                {tab === t && <motion.span layoutId="activity-tab" transition={spring} className="absolute inset-0 rounded-lg bg-white/[0.07]" />}
                <span className="relative">{t === 'actions' ? 'This session' : 'modman log'}</span>
              </button>
            ))}
          </div>
        }
      >
        Everything the interface ran, with full output. The log also includes the command line and the auto-update timer.
      </PageHeader>

      {tab === 'actions' ? (
        jobs === null ? <Skeleton className="h-96 rounded-3xl" /> : jobs.length === 0 ? (
          <EmptyState icon={<TerminalWindowIcon size={28} weight="duotone" />} title="Nothing has run yet">
            Deploys, installs and other actions you start here show up with their output.
          </EmptyState>
        ) : (
          <div className="grid gap-5 lg:grid-cols-[minmax(0,320px)_1fr]">
            <ul className="grid content-start gap-1">
              {jobs.map((j) => (
                <li key={j.id}>
                  <button
                    onClick={() => setSelected(j.id)}
                    className={cx('flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors', selected === j.id ? 'bg-white/[0.06]' : 'hover:bg-white/[0.03]')}
                  >
                    <StatusDot tone={j.status === 'running' ? 'accent' : j.status === 'done' ? 'zinc' : 'rose'} live={j.status === 'running'} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[14px] text-zinc-200">{j.label}</span>
                      <span className="block font-mono text-[11px] text-zinc-500">{j.status} · {ago(j.started)}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
            <div className="min-h-[420px] rounded-3xl border border-white/[0.05] bg-ink-2 p-5">
              <ConsoleView lines={liveSelected && live ? live.lines : output} className="h-[60dvh]" />
            </div>
          </div>
        )
      ) : (
        <div className="rounded-3xl border border-white/[0.05] bg-ink-2 p-5">
          <div className="mb-3 flex items-center justify-between gap-3">
            <span className="truncate font-mono text-[11.5px] text-zinc-500">{log?.file}</span>
            <Button size="sm" variant="ghost" icon={<ArrowClockwiseIcon size={14} />} onClick={loadLog}>Reload</Button>
          </div>
          {log ? <ConsoleView lines={log.lines} className="h-[64dvh]" /> : <Skeleton className="h-[64dvh]" />}
        </div>
      )}
    </div>
  )
}
