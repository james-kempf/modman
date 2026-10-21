import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { api, ApiError, followJob, type JobStatus, type JobSummary, type UiState } from './api'

export type View = 'overview' | 'mods' | 'collections' | 'requirements' | 'install' | 'activity' | 'settings'
const VIEWS: View[] = ['overview', 'mods', 'collections', 'requirements', 'install', 'activity', 'settings']

export interface RunOptions {
  label?: string
  game?: string | null      // defaults to the selected game; null = no game
  key?: string              // only for login (sent as stdin, never stored)
  quiet?: boolean           // do not pop the console on failure
}

export interface LiveJob {
  summary: JobSummary | null
  label: string
  lines: string[]
  status: JobStatus | 'queued'
  code: number | null
  queued: number
  error?: string
}

interface AppCtx {
  state: UiState | null
  loading: boolean
  error: string | null
  game: string | null
  setGame: (d: string) => void
  refresh: () => Promise<void>
  thumbs: Record<string, string>
  view: View
  go: (v: View) => void
  run: (args: string[], opts?: RunOptions) => Promise<JobStatus>
  busy: boolean
  jobVersion: number
  pending: Record<string, boolean>
  setPending: (ids: string[], value: boolean | null) => void
  consoleOpen: boolean
  setConsoleOpen: (open: boolean) => void
}

const Ctx = createContext<AppCtx | null>(null)
const JobCtx = createContext<LiveJob | null>(null)

const readHash = (): View => {
  const v = window.location.hash.replace(/^#\/?/, '') as View
  return VIEWS.includes(v) ? v : 'overview'
}

interface Task { args: string[]; opts: RunOptions; resolve: (s: JobStatus) => void }

export function AppProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<UiState | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [game, setGameState] = useState<string | null>(() => {
    try { return localStorage.getItem('modman.game') } catch { return null }
  })
  const [thumbs, setThumbs] = useState<Record<string, string>>({})
  const [view, setView] = useState<View>(readHash)
  const [job, setJob] = useState<LiveJob | null>(null)
  const [jobVersion, setJobVersion] = useState(0)
  const [pending, setPendingMap] = useState<Record<string, boolean>>({})
  const [consoleOpen, setConsoleOpen] = useState(false)
  const queue = useRef<Task[]>([])
  const working = useRef(false)
  const gameRef = useRef(game)
  gameRef.current = game

  const refresh = useCallback(async () => {
    try {
      const s = await api.state(gameRef.current)
      setState(s)
      setError(null)
      if (s.selectedGame && s.selectedGame !== gameRef.current) {
        gameRef.current = s.selectedGame
        setGameState(s.selectedGame)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void refresh() }, [refresh, game])

  // Thumbnails load after the state, without blocking anything.
  useEffect(() => {
    if (!state?.selectedGame) return
    let alive = true
    api.thumbs(state.selectedGame).then((t) => alive && setThumbs(t)).catch(() => {})
    return () => { alive = false }
  }, [state?.selectedGame, state?.mods.length])

  useEffect(() => {
    const onHash = () => setView(readHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const go = useCallback((v: View) => {
    if (window.location.hash !== `#/${v}`) window.location.hash = `/${v}`
    setView(v)
  }, [])

  const setGame = useCallback((d: string) => {
    try { localStorage.setItem('modman.game', d) } catch { /* private mode */ }
    gameRef.current = d
    setThumbs({})
    setGameState(d)
  }, [])

  const setPending = useCallback((ids: string[], value: boolean | null) => {
    setPendingMap((m) => {
      const next = { ...m }
      for (const id of ids) {
        if (value === null) delete next[id]
        else next[id] = value
      }
      return next
    })
  }, [])

  const pump = useCallback(async () => {
    if (working.current) return
    working.current = true
    while (queue.current.length) {
      const task = queue.current.shift()!
      const label = task.opts.label ?? task.args.join(' ')
      setJob({ summary: null, label, lines: [], status: 'running', code: null, queued: queue.current.length })
      let final: JobStatus = 'failed'
      try {
        let summary: JobSummary | null = null
        for (let attempt = 0; !summary; attempt++) {
          try {
            summary = await api.startJob({
              args: task.args,
              game: task.opts.game === undefined ? gameRef.current : task.opts.game,
              label,
              key: task.opts.key,
            })
          } catch (e) {
            // 409: something else (another tab, the auto-update timer) is running — wait for it
            if (e instanceof ApiError && e.status === 409 && attempt < 120) {
              await new Promise((r) => setTimeout(r, 1000))
              continue
            }
            throw e
          }
        }
        setJob((j) => j && { ...j, summary })
        const end = await followJob(summary.id, (lines) =>
          setJob((j) => j && { ...j, lines: j.lines.length > 4000 ? [...j.lines.slice(-3000), ...lines] : [...j.lines, ...lines] }))
        final = end.status
        setJob((j) => j && { ...j, status: end.status, code: end.code, queued: queue.current.length })
        if (end.status === 'failed' && !task.opts.quiet) setConsoleOpen(true)
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setJob((j) => j && { ...j, status: 'failed', error: msg, lines: [...j.lines, msg] })
        if (!task.opts.quiet) setConsoleOpen(true)
      }
      await refresh()
      setJobVersion((v) => v + 1)
      task.resolve(final)
    }
    working.current = false
  }, [refresh])

  const run = useCallback((args: string[], opts: RunOptions = {}) =>
    new Promise<JobStatus>((resolve) => {
      queue.current.push({ args, opts, resolve })
      setJob((j) => (j && j.status === 'running' ? { ...j, queued: queue.current.length } : j))
      void pump()
    }), [pump])

  const busy = job?.status === 'running'

  const value = useMemo<AppCtx>(() => ({
    state, loading, error, game, setGame, refresh, thumbs, view, go, run, busy, jobVersion,
    pending, setPending, consoleOpen, setConsoleOpen,
  }), [state, loading, error, game, setGame, refresh, thumbs, view, go, run, busy, jobVersion, pending, setPending, consoleOpen])

  return (
    <Ctx.Provider value={value}>
      <JobCtx.Provider value={job}>{children}</JobCtx.Provider>
    </Ctx.Provider>
  )
}

export function useApp(): AppCtx {
  const c = useContext(Ctx)
  if (!c) throw new Error('useApp outside AppProvider')
  return c
}

export const useLiveJob = () => useContext(JobCtx)

export function useCurrentGame() {
  const { state } = useApp()
  return state?.games.find((g) => g.domain === state.selectedGame) ?? null
}
