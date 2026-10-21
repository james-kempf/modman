import { memo, useEffect, useMemo, useState, type ReactNode } from 'react'
import { AnimatePresence, motion, Reorder, useDragControls } from 'framer-motion'
import {
  ArrowLineDownIcon, ArrowLineUpIcon, ArrowSquareOutIcon, ArrowsClockwiseIcon, CaretDownIcon, DotsSixVerticalIcon,
  MagnifyingGlassIcon, RocketLaunchIcon, StackIcon, TrashIcon,
} from '@phosphor-icons/react'
import type { Mod } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { bytes, cx, dateTime, plural } from '../lib/format'
import { Button, Chip, ConfirmDialog, EmptyState, PageHeader, spring, Thumb, Toggle } from '../components/ui'

type Filter = 'all' | 'enabled' | 'disabled'

export function Mods() {
  const { state, run, busy, thumbs, pending, setPending, go } = useApp()
  const game = useCurrentGame()
  const [text, setText] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [col, setCol] = useState('')
  const [open, setOpen] = useState<string | null>(null)
  const [order, setOrder] = useState<string[]>([])
  const [confirm, setConfirm] = useState<Mod | null>(null)

  const mods = state?.mods ?? []
  const byId = useMemo(() => new Map(mods.map((m) => [m.id, m])), [mods])
  useEffect(() => { setOrder(mods.map((m) => m.id)) }, [mods])

  const isOn = (m: Mod) => pending[m.id] ?? m.enabled
  const q = text.trim().toLowerCase()
  const filtering = !!q || filter !== 'all' || !!col
  const visible = useMemo(() => mods.filter((m) =>
    (!q || m.name.toLowerCase().includes(q) || (m.author ?? '').toLowerCase().includes(q) || String(m.modId ?? '').includes(q))
    && (filter === 'all' || (filter === 'enabled') === (pending[m.id] ?? m.enabled))
    && (!col || (m.collections ?? []).includes(col)),
  ), [mods, q, filter, col, pending])

  if (!state || !game) return null

  const toggle = async (m: Mod, value: boolean) => {
    setPending([m.id], value)
    await run([value ? 'enable' : 'disable', m.id], { label: `${value ? 'Enable' : 'Disable'} ${m.name}` })
    setPending([m.id], null)
  }

  const commitMove = (id: string) => {
    const m = byId.get(id)
    const idx = order.indexOf(id)
    if (!m || idx < 0 || idx + 1 === m.position) return
    run(['order', id, String(idx + 1)], { label: `Move ${m.name} to #${idx + 1}` })
  }

  const rowProps = (m: Mod) => ({
    mod: m,
    on: isOn(m),
    thumb: m.modId ? thumbs[String(m.modId)] : undefined,
    open: open === m.id,
    site: state.site,
    domain: game.domain,
    busy,
    onOpen: () => setOpen((o) => (o === m.id ? null : m.id)),
    onToggle: (v: boolean) => toggle(m, v),
    onMove: (pos: number) => run(['order', m.id, String(pos)], { label: `Move ${m.name} to #${pos}` }),
    onReinstall: () => run(['reinstall', m.id], { label: `Reinstall ${m.name}` }),
    onUninstall: () => setConfirm(m),
    total: mods.length,
  })

  return (
    <div>
      <PageHeader
        eyebrow={game.name}
        title="Mods"
        actions={
          <>
            <Button size="sm" variant="ghost" disabled={busy || !mods.length} onClick={() => run(['disable', '--all'], { label: 'Disable all mods' })}>Disable all</Button>
            <Button size="sm" disabled={busy || !mods.length} onClick={() => run(['enable', '--all'], { label: 'Enable all mods' })}>Enable all</Button>
            {game.changedSinceDeploy && (
              <Button size="sm" variant="primary" disabled={busy} icon={<RocketLaunchIcon size={15} />} onClick={() => run(['deploy'], { label: `Deploy ${game.name}` })}>
                Deploy changes
              </Button>
            )}
          </>
        }
      >
        Priority runs top to bottom: when two mods ship the same file, the one lower in the list wins.
        {!filtering && mods.length > 1 && ' Drag the handle to change it.'}
      </PageHeader>

      {mods.length === 0 ? (
        <EmptyState
          icon={<StackIcon size={28} weight="duotone" />}
          title="No mods installed yet"
          action={<><Button variant="primary" onClick={() => go('install')}>Get mods</Button><Button onClick={() => go('collections')}>Install a collection</Button></>}
        >
          Install a single mod by its Nexus ID or URL, or a whole collection. They appear here in load order.
        </EmptyState>
      ) : (
        <>
          <div className="mb-5 grid gap-3 md:grid-cols-[1fr_auto_auto]">
            <label className="relative block">
              <MagnifyingGlassIcon size={17} className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-zinc-500" />
              <input className="input pl-10" placeholder="Filter by name, author or Nexus ID" value={text} onChange={(e) => setText(e.target.value)} />
            </label>
            {state.collections.length > 0 && (
              <select className="input md:w-56" value={col} onChange={(e) => setCol(e.target.value)} aria-label="Collection">
                <option value="">All collections</option>
                {state.collections.map((c) => <option key={c.slug} value={c.slug}>{c.name}</option>)}
              </select>
            )}
            <div className="flex h-11 rounded-xl border border-white/[0.06] bg-ink p-1">
              {(['all', 'enabled', 'disabled'] as Filter[]).map((f) => (
                <button
                  key={f}
                  onClick={() => setFilter(f)}
                  className={cx('relative flex-1 rounded-lg px-3.5 text-[13px] capitalize', filter === f ? 'text-zinc-50' : 'text-zinc-500 hover:text-zinc-300')}
                >
                  {filter === f && <motion.span layoutId="mods-filter" transition={spring} className="absolute inset-0 rounded-lg bg-white/[0.07]" />}
                  <span className="relative">{f}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="mb-3 flex items-center justify-between px-1 font-mono text-[11.5px] text-zinc-500">
            <span>{filtering ? `${visible.length} of ${mods.length}` : plural(mods.length, 'mod')} · {mods.filter(isOn).length} enabled</span>
            <span>{bytes(mods.reduce((s, m) => s + (m.size ?? 0), 0))} staged</span>
          </div>

          {filtering ? (
            <ul className="grid gap-2">
              {visible.map((m) => <li key={m.id} className="min-w-0"><ModRow {...rowProps(m)} /></li>)}
              {!visible.length && <li className="py-14 text-center text-[14px] text-zinc-500">Nothing matches this filter.</li>}
            </ul>
          ) : (
            <Reorder.Group axis="y" values={order} onReorder={setOrder} className="grid min-w-0 gap-2">
              {order.map((id) => {
                const m = byId.get(id)
                return m ? <DraggableRow key={id} id={id} onDrop={() => commitMove(id)} row={rowProps(m)} disabled={busy} /> : null
              })}
            </Reorder.Group>
          )}
        </>
      )}

      <ConfirmDialog
        open={!!confirm}
        title={`Uninstall ${confirm?.name ?? ''}?`}
        body="Deletes its staged files. The downloaded archive is kept, so reinstalling later does not need a new download. Deploy afterwards to remove it from the game."
        confirm="Uninstall"
        onConfirm={() => confirm && run(['uninstall', confirm.id], { label: `Uninstall ${confirm.name}` })}
        onClose={() => setConfirm(null)}
      />
    </div>
  )
}

type RowProps = {
  mod: Mod; on: boolean; thumb?: string; open: boolean; site: string; domain: string; busy: boolean; total: number
  onOpen: () => void; onToggle: (v: boolean) => void; onMove: (pos: number) => void; onReinstall: () => void; onUninstall: () => void
}

function DraggableRow({ id, row, onDrop, disabled }: { id: string; row: RowProps; onDrop: () => void; disabled: boolean }) {
  const controls = useDragControls()
  return (
    <Reorder.Item
      value={id}
      dragListener={false}
      dragControls={controls}
      onDragEnd={onDrop}
      transition={spring}
      whileDrag={{ scale: 1.01, zIndex: 5, boxShadow: '0 24px 50px -20px rgb(0 0 0 / 0.8)' }}
      className="relative min-w-0 rounded-2xl"
    >
      <ModRow
        {...row}
        handle={
          <button
            aria-label="Drag to reorder"
            disabled={disabled}
            onPointerDown={(e) => { if (!disabled) controls.start(e) }}
            className="grid h-full w-8 shrink-0 cursor-grab touch-none place-items-center text-zinc-600 hover:text-zinc-300 active:cursor-grabbing disabled:opacity-30"
          >
            <DotsSixVerticalIcon size={18} weight="bold" />
          </button>
        }
      />
    </Reorder.Item>
  )
}

const ModRow = memo(function ModRow({
  mod, on, thumb, open, site, domain, busy, total, onOpen, onToggle, onMove, onReinstall, onUninstall, handle,
}: RowProps & { handle?: ReactNode }) {
  const nexus = mod.modId ? `${site}/${domain}/mods/${mod.modId}` : null
  const choices = (mod.fomod?.options ?? []).flatMap((o) => (o.groups ?? []).map((g) => ({ step: o.name, group: g.name, picks: (g.choices ?? []).map((c) => c.name) })))
  return (
    <div className={cx('min-w-0 rounded-2xl border transition-colors duration-200', open ? 'border-white/[0.09] bg-ink-3' : 'border-white/[0.05] bg-ink-2 hover:border-white/[0.08]')}>
      <div className="flex min-h-16 items-center gap-3 py-2.5 pr-4 pl-1.5">
        {handle ?? <span className="w-1.5" />}
        <span className="w-9 shrink-0 text-right font-mono text-[12px] text-zinc-600">{mod.position}</span>
        <Thumb src={thumb} name={mod.name} className={cx('size-11 shrink-0 rounded-xl transition-opacity', !on && 'opacity-40 grayscale')} />
        <button onClick={onOpen} className="flex min-w-0 flex-1 items-center gap-3 text-left">
          <span className="min-w-0 flex-1">
            <span className={cx('block truncate text-[15px] transition-colors', on ? 'text-zinc-100' : 'text-zinc-500')}>{mod.name}</span>
            <span className="block truncate font-mono text-[11.5px] text-zinc-500">
              {mod.version || '—'}{mod.author ? ` · ${mod.author}` : ''}{mod.category ? ` · ${mod.category}` : ''}
            </span>
          </span>
          <span className="hidden items-center gap-2 md:flex">
            {(mod.collections ?? []).length > 0 && <Chip tone="accent">collection</Chip>}
            {mod.root === 'game' && <Chip>game root</Chip>}
            {mod.fomod && <Chip>fomod</Chip>}
            <span className="w-16 text-right font-mono text-[11.5px] text-zinc-500">{bytes(mod.size)}</span>
          </span>
          <CaretDownIcon size={15} className={cx('shrink-0 text-zinc-600 transition-transform duration-300', open && 'rotate-180')} />
        </button>
        <Toggle checked={on} onChange={onToggle} label={`${on ? 'Disable' : 'Enable'} ${mod.name}`} />
      </div>

      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={spring}
            className="border-t border-white/[0.05] px-5 pt-5 pb-5 md:pl-[5.5rem]"
          >
            <dl className="grid grid-cols-2 gap-x-6 gap-y-4 text-[13px] md:grid-cols-4">
              <Fact k="Nexus ID" v={mod.modId ? `${mod.modId} / file ${mod.fileId}` : 'local archive'} />
              <Fact k="Files" v={`${mod.files ?? 0} · ${bytes(mod.size)}`} />
              <Fact k="Installed" v={dateTime(mod.installed)} />
              <Fact k="Updates" v={mod.updatePolicy === 'exact' ? 'pinned by collection' : 'follow latest'} />
            </dl>
            {choices.length > 0 && (
              <div className="mt-5">
                <div className="eyebrow mb-2">Installer choices</div>
                <ul className="grid gap-1 text-[13px] text-zinc-400">
                  {choices.map((c, i) => <li key={i}><span className="text-zinc-500">{c.group}:</span> {c.picks.join(', ') || '—'}</li>)}
                </ul>
              </div>
            )}
            <div className="mt-5 flex flex-wrap gap-2">
              {nexus && <a href={nexus} target="_blank" rel="noreferrer" className="inline-flex h-9 items-center gap-1.5 rounded-xl bg-white/[0.06] px-3 text-[13px] text-zinc-200 hover:bg-white/[0.1]"><ArrowSquareOutIcon size={14} /> Nexus page</a>}
              <Button size="sm" disabled={busy || mod.position === 1} icon={<ArrowLineUpIcon size={14} />} onClick={() => onMove(1)}>Lowest priority</Button>
              <Button size="sm" disabled={busy || mod.position === total} icon={<ArrowLineDownIcon size={14} />} onClick={() => onMove(total)}>Highest priority</Button>
              <Button size="sm" disabled={busy} icon={<ArrowsClockwiseIcon size={14} />} onClick={onReinstall}>Reinstall</Button>
              <Button size="sm" variant="danger" disabled={busy} icon={<TrashIcon size={14} />} onClick={onUninstall}>Uninstall</Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
})

function Fact({ k, v }: { k: string; v: string }) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow">{k}</dt>
      <dd className="mt-1 truncate font-mono text-[12.5px] text-zinc-300">{v}</dd>
    </div>
  )
}
