// Typed access to web/server.py. Every mutating action is a modman "job" whose output streams back.

export interface Game {
  domain: string
  name: string
  folder: string
  folderExists: boolean
  profile: string
  appid: string
  method: string
  subdir: string
  pluginsFile: string
  dataDir: string
  mods: number
  enabled: number
  collections: number
  size: number
  deployed: boolean
  deployedAt: number | null
  files: number
  changedSinceDeploy: boolean
  autoupdate: { lastOk: number | null; failed: boolean; build: string | null }
}

export interface Mod {
  id: string
  name: string
  version?: string
  modId?: number | null
  fileId?: number | null
  source?: string
  archive?: string
  collections?: string[]
  author?: string
  category?: string
  updatePolicy?: string
  root?: string
  variant?: string
  fomod?: { options?: { name: string; groups?: { name: string; choices?: { name: string }[] }[] }[] } | null
  files?: number
  size?: number
  installed?: number
  enabled: boolean
  position: number
}

export interface Collection {
  slug: string
  name: string
  author: string
  domain: string
  revision: number
  installed: number
  mods: string[]
  failed: string[]
  url: string
  installedMods: number
  enabledMods: number
}

export interface UiState {
  version: string
  site: string
  dataDir: string
  configDir: string
  account: { loggedIn: boolean; user: string; premium: boolean }
  config: {
    deployMethod: string
    downloadJobs: number
    autoDeploy: boolean
    fomod: string
    autoupdateCollections: boolean
    autoupdateInterval: number
    viewAdult: boolean
  }
  autoupdate: { scheduler: string | null }
  nxmRegistered: boolean
  profiles: string[]
  activeGame: string
  selectedGame: string | null
  games: Game[]
  mods: Mod[]
  collections: Collection[]
}

export type DepStatus = 'missing' | 'disabled' | 'external' | 'gone' | 'ok'
export interface Requirement {
  status: DepStatus
  key: string
  name: string
  hint: string
  urls: string[]
  by: string[]
  detail: string
  acked: boolean
}

export interface DetectedGame { domain: string; name: string; folder: string; configured: boolean }

export interface SearchResult {
  modId: number
  name: string
  author: string
  version: string
  summary: string
  downloads: number
  endorsements: number
  thumbnailUrl?: string
  category?: string
  updatedAt?: string
  installed: boolean
}

export type JobStatus = 'running' | 'done' | 'failed' | 'cancelled'
export interface JobSummary {
  id: string
  args: string[]
  game: string | null
  label: string
  status: JobStatus
  code: number | null
  started: number
  ended: number | null
  lines: number
  last: string
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

const token = document.querySelector<HTMLMetaElement>('meta[name="modman-token"]')?.content ?? ''
const headers = { 'X-Modman-Token': token }

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, { ...init, headers: { ...headers, ...(init?.headers ?? {}) } })
  const text = await res.text()
  let body: unknown = null
  try { body = text ? JSON.parse(text) : null } catch { body = { error: text } }
  if (!res.ok) {
    const msg = (body as { error?: string } | null)?.error ?? `${res.status} ${res.statusText}`
    throw new ApiError(res.status, msg)
  }
  return body as T
}

const q = (game?: string | null, extra: Record<string, string> = {}) => {
  const p = new URLSearchParams(extra)
  if (game) p.set('game', game)
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const api = {
  state: (game?: string | null) => request<UiState>(`/api/state${q(game)}`),
  thumbs: (game: string) => request<Record<string, string>>(`/api/thumbs${q(game)}`),
  detect: () => request<DetectedGame[]>('/api/detect'),
  deps: (game: string, refresh = false) =>
    request<Requirement[]>(`/api/deps${q(game, refresh ? { refresh: '1' } : {})}`),
  search: (game: string, text: string) =>
    request<{ total: number; mods: SearchResult[] }>(`/api/search${q(game, { q: text })}`),
  log: (lines = 400) => request<{ lines: string[]; file: string }>(`/api/log?lines=${lines}`),
  jobs: () => request<{ jobs: JobSummary[]; busy: boolean }>('/api/jobs'),
  job: (id: string) => request<JobSummary & { output: string[] }>(`/api/jobs/${id}`),
  startJob: (body: { args: string[]; game?: string | null; label?: string; key?: string }) =>
    request<JobSummary>('/api/jobs', { method: 'POST', body: JSON.stringify(body), headers: { 'Content-Type': 'application/json' } }),
  cancelJob: (id: string) => request<{ cancelled: boolean }>(`/api/jobs/${id}/cancel`, { method: 'POST', body: '{}' }),
}

/**
 * Follows a job's output (server-sent events over fetch, so the token stays in a header).
 * Resolves with the final status once the job ends.
 */
export async function followJob(
  id: string,
  onLines: (lines: string[]) => void,
  signal?: AbortSignal,
): Promise<{ status: JobStatus; code: number | null }> {
  let from = 0
  for (let attempt = 0; attempt < 30; attempt++) {
    try {
      const res = await fetch(`/api/jobs/${id}/events?from=${from}`, { headers, signal })
      if (!res.ok || !res.body) throw new ApiError(res.status, 'stream failed')
      const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()
      let buf = ''
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        buf += value
        let cut: number
        while ((cut = buf.indexOf('\n\n')) >= 0) {
          const chunk = buf.slice(0, cut)
          buf = buf.slice(cut + 2)
          const event = /^event: (\w+)/m.exec(chunk)?.[1]
          const data = /^data: (.*)$/m.exec(chunk)?.[1]
          if (!event || !data) continue
          const payload = JSON.parse(data)
          if (event === 'lines') {
            from = payload.from + payload.lines.length
            onLines(payload.lines)
          } else if (event === 'end') {
            return payload
          }
        }
      }
    } catch (e) {
      if (signal?.aborted) throw e
      if (e instanceof ApiError && e.status === 404) throw e
    }
    await new Promise((r) => setTimeout(r, Math.min(400 * (attempt + 1), 3000)))
  }
  throw new ApiError(0, 'lost connection to the job')
}
