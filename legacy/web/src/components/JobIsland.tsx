import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { CheckIcon, StopIcon, WarningIcon, XIcon } from '@phosphor-icons/react'
import { api } from '../lib/api'
import { useApp, useLiveJob } from '../lib/store'
import { cx } from '../lib/format'
import { spring } from './ui'

/** Colours a console line by the markers modman prints. */
export function ConsoleLine({ text }: { text: string }) {
  const banner = /^\s*│\s*(.*?)\s*│\s*$/.exec(text)
  if (banner) return <div className="pt-1 pb-1 font-sans text-[14px] font-medium text-zinc-100">{banner[1]}</div>
  const tone =
    /^\s*\[(ERR|ERR )\]|✗|failed|Error/i.test(text) ? 'text-rose-300'
      : /^\s*\[WARN\]/.test(text) ? 'text-amber-300'
        : /^\s*\[ OK \]|✓/.test(text) ? 'text-accent-300'
          : /^\s*(──|┌|│|└)/.test(text) ? 'text-zinc-500'
            : 'text-zinc-300'
  return <div className={cx('whitespace-pre-wrap break-words', tone)}>{text || ' '}</div>
}

export function ConsoleView({ lines, follow = true, className }: { lines: string[]; follow?: boolean; className?: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const stick = useRef(true)
  useEffect(() => {
    const el = ref.current
    if (el && follow && stick.current) el.scrollTop = el.scrollHeight
  }, [lines, follow])
  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40
      }}
      className={cx('overflow-y-auto font-mono text-[12.5px] leading-[1.7]', className)}
    >
      {lines.map((l, i) => (/^\s*[┌└]─+[┐┘]\s*$/.test(l) ? null : <ConsoleLine key={i} text={l} />))}
    </div>
  )
}

/**
 * Dynamic-island style status pill. Collapsed it shows the running action and its latest line;
 * expanded (same layoutId) it becomes the live console.
 */
export function JobIsland() {
  const job = useLiveJob()
  const { consoleOpen, setConsoleOpen } = useApp()
  const [visible, setVisible] = useState(false)

  useEffect(() => {
    if (!job) return
    setVisible(true)
    if (job.status === 'done' && !consoleOpen) {
      const t = setTimeout(() => setVisible(false), 5000)
      return () => clearTimeout(t)
    }
  }, [job, job?.status, consoleOpen])

  if (!job) return null
  const running = job.status === 'running'
  const failed = job.status === 'failed' || job.status === 'cancelled'
  const last = [...job.lines].reverse().find((l) => l.trim()) ?? 'Starting…'

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-40 flex justify-center px-4 pb-5 lg:pl-[284px]">
      <AnimatePresence mode="popLayout">
        {consoleOpen ? (
          <motion.div
            key="console"
            layoutId="job-island"
            transition={spring}
            className="glass pointer-events-auto flex h-[min(62dvh,560px)] w-full max-w-4xl flex-col overflow-hidden rounded-[28px]"
          >
            <div className="flex items-center gap-3 border-b border-white/[0.06] px-5 py-3.5">
              <StateIcon running={running} failed={failed} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[14px] font-medium text-zinc-100">{job.label}</div>
                <div className="font-mono text-[11px] text-zinc-500">
                  {running ? 'running' : job.status}{job.code != null && !running ? ` · exit ${job.code}` : ''}
                  {job.queued > 0 && ` · ${job.queued} queued`}
                </div>
              </div>
              {running && job.summary && (
                <button
                  onClick={() => job.summary && api.cancelJob(job.summary.id)}
                  className="flex h-9 items-center gap-1.5 rounded-xl px-3 text-[13px] text-zinc-400 hover:bg-white/[0.06] hover:text-rose-300"
                >
                  <StopIcon size={14} weight="fill" /> Stop
                </button>
              )}
              <button
                aria-label="Close console"
                onClick={() => setConsoleOpen(false)}
                className="grid size-9 place-items-center rounded-xl text-zinc-400 hover:bg-white/[0.06] hover:text-zinc-100"
              >
                <XIcon size={16} />
              </button>
            </div>
            <ConsoleView lines={job.lines} className="flex-1 px-5 py-4" />
          </motion.div>
        ) : visible ? (
          <motion.button
            key="pill"
            layoutId="job-island"
            transition={spring}
            initial={{ opacity: 0, y: 24 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 24 }}
            onClick={() => setConsoleOpen(true)}
            className="glass pointer-events-auto relative flex h-14 w-full max-w-xl items-center gap-3 overflow-hidden rounded-full pr-5 pl-3 text-left"
          >
            {running && (
              <span className="pointer-events-none absolute inset-0 overflow-hidden rounded-full">
                <span className="absolute inset-y-0 w-1/2 animate-shimmer bg-gradient-to-r from-transparent via-accent-400/[0.08] to-transparent" />
              </span>
            )}
            <StateIcon running={running} failed={failed} />
            <span className="relative min-w-0 flex-1">
              <span className="block truncate text-[13.5px] font-medium text-zinc-100">{job.label}</span>
              <span className="block truncate font-mono text-[11px] text-zinc-500">{running ? last : failed ? 'Failed — tap for details' : 'Finished'}</span>
            </span>
            {job.queued > 0 && <span className="relative font-mono text-[11px] text-zinc-400">+{job.queued}</span>}
          </motion.button>
        ) : null}
      </AnimatePresence>
    </div>
  )
}

function StateIcon({ running, failed }: { running: boolean; failed: boolean }) {
  return (
    <span
      className={cx(
        'relative grid size-9 shrink-0 place-items-center rounded-full',
        running ? 'bg-accent-400/15 text-accent-300' : failed ? 'bg-rose-400/15 text-rose-300' : 'bg-accent-400/15 text-accent-300',
      )}
    >
      {running ? (
        <span className="flex gap-[3px]">
          {[0, 1, 2].map((i) => (
            <motion.span
              key={i}
              className="h-3.5 w-[3px] rounded-full bg-current"
              animate={{ scaleY: [0.35, 1, 0.35] }}
              transition={{ duration: 0.9, repeat: Infinity, delay: i * 0.15, ease: 'easeInOut' }}
            />
          ))}
        </span>
      ) : failed ? <WarningIcon size={17} weight="bold" /> : <CheckIcon size={17} weight="bold" />}
    </span>
  )
}
