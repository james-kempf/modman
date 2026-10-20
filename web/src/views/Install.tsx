import { useState } from 'react'
import { motion } from 'framer-motion'
import { CheckIcon, DownloadSimpleIcon, MagnifyingGlassIcon, ThumbsUpIcon } from '@phosphor-icons/react'
import { api, type SearchResult } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { compact, cx } from '../lib/format'
import { Button, EmptyState, ErrorNote, Field, PageHeader, Skeleton, quick, Thumb } from '../components/ui'

const REF_RE = /^(\d+|[a-z0-9]+:\d+|nxm:\/\/.+|https?:\/\/(www\.)?nexusmods\.com\/.+|\/.+|~\/.+)$/i

export function Install() {
  const { state, run, busy } = useApp()
  const game = useCurrentGame()
  const [ref, setRef] = useState('')
  const [refErr, setRefErr] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<SearchResult[] | null>(null)
  const [total, setTotal] = useState(0)
  const [searching, setSearching] = useState(false)
  const [searchErr, setSearchErr] = useState<string | null>(null)
  const [installing, setInstalling] = useState<Record<number, boolean>>({})

  if (!state || !game) return null

  const install = () => {
    const v = ref.trim()
    if (!REF_RE.test(v)) { setRefErr('Use a Nexus mod ID, a nexusmods.com link, an nxm:// link, or an absolute path to an archive.'); return }
    setRefErr(null)
    run(['install', v], { label: `Install ${v.length > 48 ? `${v.slice(0, 48)}…` : v}` })
    setRef('')
  }

  const search = async () => {
    const t = query.trim()
    if (!t) return
    setSearching(true)
    setSearchErr(null)
    try {
      const r = await api.search(game.domain, t)
      setResults(r.mods)
      setTotal(r.total)
    } catch (e) {
      setSearchErr(e instanceof Error ? e.message : String(e))
    } finally {
      setSearching(false)
    }
  }

  const installResult = async (m: SearchResult) => {
    setInstalling((s) => ({ ...s, [m.modId]: true }))
    const status = await run(['install', String(m.modId)], { label: `Install ${m.name}` })
    setInstalling((s) => ({ ...s, [m.modId]: false }))
    if (status === 'done') setResults((rs) => rs?.map((r) => (r.modId === m.modId ? { ...r, installed: true } : r)) ?? null)
  }

  return (
    <div>
      <PageHeader eyebrow={game.name} title="Get mods">
        Install by ID or link, or search Nexus Mods. The main file is picked automatically; installers use their defaults.
      </PageHeader>

      <section className="grid gap-6 lg:grid-cols-[1fr_1.15fr]">
        <div className="rounded-[2rem] border border-white/[0.06] bg-ink-2 p-6 md:p-8">
          <div className="eyebrow mb-5">Direct install</div>
          <div className="grid gap-4">
            <Field label="Mod reference" error={refErr} helper="Examples: 2237 · nexusmods.com/palworld/mods/2237 · /home/you/Downloads/Mod.zip">
              {(id) => (
                <input id={id} className="input font-mono text-[14px]" placeholder="Nexus ID, link, or archive path"
                  value={ref} onChange={(e) => setRef(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && install()} />
              )}
            </Field>
            <Button variant="primary" disabled={busy || !ref.trim()} icon={<DownloadSimpleIcon size={16} />} onClick={install}>Install</Button>
            {!state.account.premium && state.account.loggedIn && (
              <p className="text-[12.5px] leading-relaxed text-amber-200/80">
                Free accounts need a click on &ldquo;Slow download&rdquo; on the website for each file. Use the Mod Manager Download button there with the nxm handler registered (Settings).
              </p>
            )}
          </div>
        </div>

        <div className="rounded-[2rem] border border-white/[0.06] bg-ink-2 p-6 md:p-8">
          <div className="eyebrow mb-5">Search Nexus Mods</div>
          <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); void search() }}>
            <label className="relative flex-1">
              <MagnifyingGlassIcon size={17} className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-zinc-500" />
              <input className="input pl-10" placeholder={`Search ${game.name} mods`} value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Search" />
            </label>
            <Button type="submit" disabled={searching || !query.trim()}>Search</Button>
          </form>
          <p className="mt-3 text-[12.5px] text-zinc-500">{results ? `${total.toLocaleString()} results, best matches first` : 'Matches mod names, most relevant first.'}</p>
        </div>
      </section>

      <section className="mt-10">
        {searchErr && <ErrorNote>Search failed: {searchErr}</ErrorNote>}
        {searching ? (
          <div className="grid gap-4 md:grid-cols-2">{[0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-36 rounded-3xl" />)}</div>
        ) : results && results.length === 0 ? (
          <EmptyState icon={<MagnifyingGlassIcon size={28} />} title="No mods found">Try fewer words, or the mod&apos;s exact name.</EmptyState>
        ) : results ? (
          <ul className="grid gap-4 md:grid-cols-2">
            {results.map((m, i) => (
              <motion.li
                key={m.modId}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ ...quick, delay: Math.min(i, 12) * 0.015 }}
                className={cx('flex gap-4 rounded-3xl border border-white/[0.05] bg-ink-2 p-4', i % 5 === 0 && 'md:col-span-2')}
              >
                <Thumb src={m.thumbnailUrl} name={m.name} className={cx('shrink-0 rounded-2xl', i % 5 === 0 ? 'h-28 w-44' : 'size-24')} />
                <div className="flex min-w-0 flex-1 flex-col">
                  <div className="flex items-start gap-2">
                    <div className="min-w-0 flex-1">
                      <a href={`${state.site}/${game.domain}/mods/${m.modId}`} target="_blank" rel="noreferrer" className="block truncate text-[15.5px] font-medium text-zinc-100 hover:text-accent-200">{m.name}</a>
                      <div className="truncate font-mono text-[11.5px] text-zinc-500">v{m.version.replace(/^v/i, '')} · {m.author} · {m.category}</div>
                    </div>
                  </div>
                  <p className="mt-2 line-clamp-2 text-[13px] leading-relaxed text-zinc-400">{m.summary}</p>
                  <div className="mt-auto flex items-center gap-4 pt-3">
                    <span className="flex items-center gap-1.5 font-mono text-[11.5px] text-zinc-500"><DownloadSimpleIcon size={13} />{compact(m.downloads)}</span>
                    <span className="flex items-center gap-1.5 font-mono text-[11.5px] text-zinc-500"><ThumbsUpIcon size={13} />{compact(m.endorsements)}</span>
                    <span className="ml-auto">
                      {m.installed ? (
                        <span className="flex h-9 items-center gap-1.5 px-2 text-[13px] text-accent-300"><CheckIcon size={14} weight="bold" /> Installed</span>
                      ) : (
                        <Button size="sm" variant="primary" disabled={installing[m.modId]} onClick={() => installResult(m)}>
                          {installing[m.modId] ? 'Installing…' : 'Install'}
                        </Button>
                      )}
                    </span>
                  </div>
                </div>
              </motion.li>
            ))}
          </ul>
        ) : null}
      </section>
    </div>
  )
}
