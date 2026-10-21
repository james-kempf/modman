import { useState } from 'react'
import { motion } from 'framer-motion'
import { ArrowSquareOutIcon, ArrowsClockwiseIcon, BooksIcon, TrashIcon, WarningIcon } from '@phosphor-icons/react'
import type { Collection } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { ago } from '../lib/format'
import { Button, Chip, ConfirmDialog, EmptyState, Field, PageHeader, quick, Toggle } from '../components/ui'

const COLLECTION_RE = /collections\/[a-z0-9]+|^[a-z0-9]{6}$/i

export function Collections() {
  const { state, run, busy } = useApp()
  const game = useCurrentGame()
  const [url, setUrl] = useState('')
  const [optional, setOptional] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [removing, setRemoving] = useState<Collection | null>(null)

  if (!state || !game) return null

  const install = () => {
    const v = url.trim()
    if (!COLLECTION_RE.test(v)) { setErr('Paste a collection URL (…/collections/abc123) or its 6-letter slug.'); return }
    setErr(null)
    const ref = /^[a-z0-9]{6}$/i.test(v) ? `nxm://${game.domain}/collections/${v}` : v
    run(['collection', 'install', ref, '--optional', optional ? 'all' : 'none'], { label: `Install collection ${v.split('/').pop()}` })
    setUrl('')
  }

  return (
    <div>
      <PageHeader eyebrow={game.name} title="Collections">
        A collection installs a curated set of mods with the curator&apos;s installer choices and load order.
      </PageHeader>

      <section className="mb-12 grid gap-6 rounded-[2rem] border border-white/[0.06] bg-ink-2 p-6 md:grid-cols-[1.4fr_1fr] md:p-8">
        <div className="grid gap-4">
          <Field label="Collection URL" helper={<>From the collection page on nexusmods.com, e.g. <span className="font-mono">nexusmods.com/games/{game.domain}/collections/rpy5tv</span></>} error={err}>
            {(id) => (
              <input
                id={id}
                className="input"
                placeholder={`https://www.nexusmods.com/games/${game.domain}/collections/…`}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && install()}
              />
            )}
          </Field>
        </div>
        <div className="flex flex-col justify-between gap-4">
          <label className="flex items-center justify-between gap-4 rounded-2xl border border-white/[0.05] bg-ink px-4 py-3">
            <span>
              <span className="block text-[14px] text-zinc-200">Include optional mods</span>
              <span className="block text-[12.5px] text-zinc-500">Otherwise only the required ones</span>
            </span>
            <Toggle checked={optional} onChange={setOptional} label="Include optional mods" />
          </label>
          <Button variant="primary" disabled={busy || !url.trim()} onClick={install}>Install collection</Button>
        </div>
      </section>

      {state.collections.length === 0 ? (
        <EmptyState icon={<BooksIcon size={28} weight="duotone" />} title="No collections installed">
          Paste a collection link above. modman downloads every file, replays the curator&apos;s FOMOD choices and applies the load-order rules.
        </EmptyState>
      ) : (
        <ul className="grid gap-5">
          {state.collections.map((c, i) => (
            <motion.li
              key={c.slug}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ ...quick, delay: Math.min(i, 8) * 0.015 }}
              className="grid gap-6 rounded-[2rem] border border-white/[0.06] bg-ink-2 p-6 md:grid-cols-[auto_1fr_auto] md:items-center md:p-8"
            >
              <Monogram name={c.name} />
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="truncate text-xl font-medium tracking-tight text-zinc-50">{c.name}</h2>
                  <Chip>rev {c.revision}</Chip>
                </div>
                <p className="mt-1 text-[13.5px] text-zinc-500">by {c.author} · installed {ago(c.installed)}</p>
                <div className="mt-4 flex items-center gap-3">
                  <div className="h-1.5 w-40 overflow-hidden rounded-full bg-white/[0.06]">
                    <div className="h-full rounded-full bg-accent-400" style={{ width: `${c.installedMods ? (c.enabledMods / c.installedMods) * 100 : 0}%` }} />
                  </div>
                  <span className="font-mono text-[12px] text-zinc-400">{c.enabledMods}/{c.installedMods} enabled</span>
                </div>
                {c.failed.length > 0 && (
                  <div className="mt-4 rounded-xl border border-amber-400/15 bg-amber-400/[0.05] px-3.5 py-2.5 text-[13px] text-amber-200">
                    <div className="flex items-center gap-2 font-medium"><WarningIcon size={14} /> {c.failed.length} mod(s) did not install</div>
                    <div className="mt-1 text-amber-200/70">{c.failed.join(', ')}</div>
                  </div>
                )}
              </div>
              <div className="flex flex-wrap gap-2 md:flex-col md:items-stretch">
                <Button size="sm" variant="primary" disabled={busy} icon={<ArrowsClockwiseIcon size={14} />}
                  onClick={() => run(['collection', 'update', c.slug, '--optional', 'none'], { label: `Update ${c.name}` })}>
                  {c.failed.length ? 'Retry / update' : 'Update'}
                </Button>
                <div className="flex gap-2">
                  <Button size="sm" className="flex-1" disabled={busy} onClick={() => run(['enable', '--collection', c.slug], { label: `Enable ${c.name}` })}>Enable</Button>
                  <Button size="sm" className="flex-1" variant="ghost" disabled={busy} onClick={() => run(['disable', '--collection', c.slug], { label: `Disable ${c.name}` })}>Disable</Button>
                </div>
                <div className="flex gap-2">
                  <a href={c.url} target="_blank" rel="noreferrer" className="inline-flex h-9 flex-1 items-center justify-center gap-1.5 rounded-xl bg-white/[0.06] px-3 text-[13px] text-zinc-200 hover:bg-white/[0.1]">
                    <ArrowSquareOutIcon size={14} /> Nexus
                  </a>
                  <Button size="sm" variant="danger" disabled={busy} icon={<TrashIcon size={14} />} onClick={() => setRemoving(c)} aria-label={`Remove ${c.name}`} />
                </div>
              </div>
            </motion.li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={!!removing}
        title={`Remove ${removing?.name ?? ''}?`}
        body="Uninstalls the collection's mods (mods shared with another collection stay). Deploy afterwards to remove them from the game."
        confirm="Remove collection"
        onConfirm={() => removing && run(['collection', 'remove', removing.slug], { label: `Remove ${removing.name}` })}
        onClose={() => setRemoving(null)}
      />
    </div>
  )
}

function Monogram({ name }: { name: string }) {
  const hue = [...name].reduce((h, c) => (h * 33 + c.charCodeAt(0)) % 360, 11)
  const letters = name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase()
  return (
    <div
      className="relative grid size-20 place-items-center overflow-hidden rounded-3xl text-xl font-semibold tracking-tight text-white/85"
      style={{ background: `linear-gradient(140deg, oklch(0.42 0.06 ${hue}), oklch(0.22 0.03 ${(hue + 50) % 360}))` }}
    >
      <span className="absolute inset-0 shadow-[inset_0_1px_0_rgb(255_255_255/0.15)]" />
      {letters}
    </div>
  )
}
