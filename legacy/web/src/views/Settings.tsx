import { useEffect, useState, type ReactNode } from 'react'
import { CheckIcon, GameControllerIcon, KeyIcon, PlusIcon } from '@phosphor-icons/react'
import { api, type DetectedGame } from '../lib/api'
import { useApp, useCurrentGame } from '../lib/store'
import { ago, cx } from '../lib/format'
import { Button, Chip, ConfirmDialog, Field, PageHeader, Skeleton, Toggle } from '../components/ui'

const SECTIONS = [
  ['account', 'Nexus account'],
  ['game', 'This game'],
  ['games', 'Add a game'],
  ['deploy', 'Deployment'],
  ['autoupdate', 'Auto-update'],
  ['system', 'Integration'],
] as const

export function Settings() {
  const { state, busy } = useApp()
  const game = useCurrentGame()
  if (!state) return null
  return (
    <div>
      <PageHeader title="Settings">Changes are saved by modman as soon as you make them.</PageHeader>
      <div className="grid gap-10 lg:grid-cols-[200px_1fr]">
        <nav className="hidden lg:block">
          <ul className="sticky top-8 grid gap-1">
            {SECTIONS.filter(([id]) => id !== 'game' || game).map(([id, label]) => (
              <li key={id}><a href={`#${id}`} onClick={(e) => { e.preventDefault(); document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' }) }} className="block rounded-lg px-3 py-2 text-[13.5px] text-zinc-500 hover:bg-white/[0.03] hover:text-zinc-200">{label}</a></li>
            ))}
          </ul>
        </nav>
        <div className="grid gap-14">
          <Account />
          {game && <GameSettings key={game.domain} />}
          <AddGame />
          <Deployment />
          <AutoUpdate />
          <Integration busy={busy} />
        </div>
      </div>
    </div>
  )
}

function Section({ id, title, children, intro }: { id: string; title: string; intro?: ReactNode; children: ReactNode }) {
  return (
    <section id={id} className="scroll-mt-8">
      <h2 className="text-[19px] font-medium tracking-tight text-zinc-50">{title}</h2>
      {intro && <p className="mt-1.5 max-w-[65ch] text-[14px] leading-relaxed text-zinc-500">{intro}</p>}
      <div className="mt-5 divide-y divide-white/[0.05] rounded-3xl border border-white/[0.05] bg-ink-2">{children}</div>
    </section>
  )
}

function Row({ title, desc, children }: { title: string; desc?: ReactNode; children: ReactNode }) {
  return (
    <div className="grid gap-4 px-6 py-5 md:grid-cols-[1fr_minmax(0,320px)] md:items-center">
      <div className="min-w-0">
        <div className="text-[14.5px] text-zinc-100">{title}</div>
        {desc && <div className="mt-1 text-[13px] leading-relaxed text-zinc-500">{desc}</div>}
      </div>
      <div className="flex min-w-0 items-center gap-2 md:justify-end">{children}</div>
    </div>
  )
}

export function Account() {
  const { state, run, busy } = useApp()
  const [key, setKey] = useState('')
  const [err, setErr] = useState<string | null>(null)
  if (!state) return null
  const signIn = async () => {
    if (key.trim().length < 10) { setErr('Paste the full personal API key.'); return }
    setErr(null)
    const s = await run(['login'], { label: 'Sign in to Nexus Mods', key: key.trim(), game: null })
    if (s === 'done') setKey('')
    else setErr('The key was rejected. Check it on your Nexus account page.')
  }
  return (
    <Section id="account" title="Nexus account" intro="modman signs in with your personal API key. It is stored only in your modman config file.">
      {state.account.loggedIn && (
        <Row title={state.account.user || 'Signed in'} desc={state.account.premium ? 'Premium: direct and parallel downloads.' : 'Free account: each download needs a click on the website.'}>
          <Chip tone={state.account.premium ? 'accent' : 'zinc'}>{state.account.premium ? 'premium' : 'free'}</Chip>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => run(['logout'], { label: 'Sign out', game: null })}>Sign out</Button>
        </Row>
      )}
      <div className="grid gap-4 px-6 py-5">
        <Field
          label={state.account.loggedIn ? 'Replace API key' : 'Personal API key'}
          error={err}
          helper={<>Copy it from <a className="text-accent-300 hover:text-accent-200" href="https://www.nexusmods.com/users/myaccount?tab=api" target="_blank" rel="noreferrer">nexusmods.com → Account → API keys</a>.</>}
        >
          {(id) => (
            <div className="flex gap-2">
              <input id={id} type="password" autoComplete="off" className="input font-mono" value={key} onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && signIn()} placeholder="••••••••••••••••" />
              <Button variant="primary" disabled={busy || !key} icon={<KeyIcon size={15} />} onClick={signIn}>Sign in</Button>
            </div>
          )}
        </Field>
      </div>
    </Section>
  )
}

function GameSettings() {
  const { state, run, busy } = useApp()
  const game = useCurrentGame()!
  const [folder, setFolder] = useState(game.folder)
  const [appid, setAppid] = useState(game.appid)
  const [removing, setRemoving] = useState(false)
  if (!state) return null
  const set = (k: string, v: string, label: string) => run(['game', 'set', k, v], { label })
  return (
    <Section id="game" title={game.name} intro={<>Nexus domain <span className="font-mono text-zinc-400">{game.domain}</span> · data in <span className="font-mono text-zinc-400">{game.dataDir}</span></>}>
      <div className="grid gap-4 px-6 py-5">
        <Field label="Game folder" helper={game.deployed ? 'Purge the deployment before moving the folder.' : 'The folder that contains the game executable.'} error={game.folderExists ? null : 'This folder does not exist (an unmounted SD card?).'}>
          {(id) => (
            <div className="flex gap-2">
              <input id={id} className="input font-mono text-[13.5px]" value={folder} onChange={(e) => setFolder(e.target.value)} disabled={game.deployed} />
              <Button disabled={busy || game.deployed || folder === game.folder || !folder.startsWith('/')} onClick={() => set('folder', folder, 'Change game folder')}>Save</Button>
            </div>
          )}
        </Field>
      </div>
      <Row title="Deploy method" desc="Hardlinks are instant and use no space; copies work across drives.">
        <select className="input md:w-52" value={game.method} disabled={busy} onChange={(e) => set('method', e.target.value, `Deploy method: ${e.target.value || 'global default'}`)}>
          <option value="">Global default ({state.config.deployMethod})</option>
          <option value="auto">Auto</option>
          <option value="hardlink">Hardlink</option>
          <option value="symlink">Symlink</option>
          <option value="copy">Copy</option>
        </select>
      </Row>
      <Row title="Steam app ID" desc="Used to launch the game and to notice Steam updates.">
        <input className="input font-mono md:w-36" value={appid} onChange={(e) => setAppid(e.target.value.replace(/\D/g, ''))} />
        <Button size="sm" disabled={busy || appid === game.appid} onClick={() => set('appid', appid, 'Change Steam app ID')}>Save</Button>
      </Row>
      <Row title="Default game" desc="The game the command line uses when you do not pass -g.">
        {state.activeGame === game.domain ? <Chip tone="accent"><CheckIcon size={12} /> default</Chip>
          : <Button size="sm" disabled={busy} onClick={() => run(['game', 'use', game.domain], { label: `Make ${game.name} the default`, game: null })}>Make default</Button>}
      </Row>
      <Row title="Forget this game" desc={game.deployed ? 'Purge it first.' : 'Removes the game from modman. Installed mods stay on disk.'}>
        <Button size="sm" variant="danger" disabled={busy || game.deployed} onClick={() => setRemoving(true)}>Remove</Button>
      </Row>
      <ConfirmDialog
        open={removing} title={`Forget ${game.name}?`} confirm="Remove"
        body="modman stops managing this game. Its staged mods and downloads stay on disk, so adding it again restores everything."
        onConfirm={() => run(['game', 'remove', game.domain], { label: `Remove ${game.name}`, game: null })}
        onClose={() => setRemoving(false)}
      />
    </Section>
  )
}

export function AddGame() {
  const { run, busy, setGame, jobVersion } = useApp()
  const [found, setFound] = useState<DetectedGame[] | null>(null)
  const [domain, setDomain] = useState('')
  const [path, setPath] = useState('')
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => { api.detect().then(setFound).catch(() => setFound([])) }, [jobVersion])

  const add = async (d: string, p?: string) => {
    const s = await run(['game', 'add', d, ...(p ? [p] : [])], { label: `Add ${d}`, game: null })
    if (s === 'done') setGame(d)
  }
  const manual = () => {
    const d = domain.trim().toLowerCase()
    if (!/^[a-z0-9]+$/.test(d)) { setErr('Use the name from the Nexus URL, e.g. stardewvalley in nexusmods.com/stardewvalley.'); return }
    if (path && !path.startsWith('/')) { setErr('The folder must be an absolute path.'); return }
    setErr(null)
    void add(d, path.trim() || undefined)
  }

  return (
    <Section id="games" title="Add a game" intro="Supported games installed through Steam are found automatically. Any other Nexus game works with its folder.">
      {found === null ? (
        <div className="grid gap-3 px-6 py-5"><Skeleton className="h-12" /><Skeleton className="h-12" /></div>
      ) : found.length ? found.map((g) => (
        <Row key={g.domain} title={g.name} desc={<span className="font-mono">{g.folder}</span>}>
          {g.configured ? <Chip tone="accent"><CheckIcon size={12} /> added</Chip>
            : <Button size="sm" variant="primary" disabled={busy} icon={<PlusIcon size={14} />} onClick={() => add(g.domain)}>Add</Button>}
        </Row>
      )) : (
        <div className="flex items-center gap-3 px-6 py-5 text-[14px] text-zinc-500"><GameControllerIcon size={18} /> No supported Steam games found.</div>
      )}
      <div className="grid gap-4 px-6 py-5 md:grid-cols-[1fr_1.5fr_auto] md:items-end">
        <Field label="Nexus game domain" error={err}>
          {(id) => <input id={id} className="input font-mono" placeholder="stardewvalley" value={domain} onChange={(e) => setDomain(e.target.value)} />}
        </Field>
        <Field label="Game folder (optional for Steam games)">
          {(id) => <input id={id} className="input font-mono text-[13.5px]" placeholder="/path/to/game" value={path} onChange={(e) => setPath(e.target.value)} />}
        </Field>
        <Button disabled={busy || !domain} onClick={manual}>Add game</Button>
      </div>
    </Section>
  )
}

function Deployment() {
  const { state, run, busy } = useApp()
  if (!state) return null
  const c = state.config
  const set = (k: string, v: string, label: string) => run(['config', k, v], { label, game: null })
  return (
    <Section id="deploy" title="Deployment" intro="Defaults for every game.">
      <Row title="Deploy method" desc="Auto uses hardlinks when mods and game share a drive, otherwise copies.">
        <select className="input md:w-44" value={c.deployMethod} disabled={busy} onChange={(e) => set('deploy-method', e.target.value, `Deploy method: ${e.target.value}`)}>
          {['auto', 'hardlink', 'symlink', 'copy'].map((m) => <option key={m} value={m}>{m}</option>)}
        </select>
      </Row>
      <Row title="Deploy after every change" desc="Enable, disable, install and reorder apply to the game immediately.">
        <Toggle checked={c.autoDeploy} disabled={busy} label="Deploy after every change" onChange={(v) => set('auto-deploy', String(v), `Auto-deploy ${v ? 'on' : 'off'}`)} />
      </Row>
      <Row title="Parallel downloads" desc="Premium accounts only.">
        <select className="input md:w-28" value={c.downloadJobs} disabled={busy} onChange={(e) => set('jobs', e.target.value, `Parallel downloads: ${e.target.value}`)}>
          {[1, 2, 3, 4, 6, 8].map((n) => <option key={n} value={n}>{n}</option>)}
        </select>
      </Row>
      <Row title="Installer choices" desc="For single mods with a FOMOD installer. The web interface always uses defaults; ask applies on the command line.">
        <select className="input md:w-44" value={c.fomod} disabled={busy} onChange={(e) => set('fomod', e.target.value, `FOMOD mode: ${e.target.value}`)}>
          <option value="auto">auto</option>
          <option value="ask">ask</option>
          <option value="default">default</option>
        </select>
      </Row>
    </Section>
  )
}

function AutoUpdate() {
  const { state, run, busy } = useApp()
  const game = useCurrentGame()
  if (!state) return null
  const on = !!state.autoupdate.scheduler
  const set = (k: string, v: string, label: string) => run(['config', k, v], { label, game: null })
  return (
    <Section id="autoupdate" title="Auto-update" intro="After an OS update or a Steam update of a game, modman installs mod updates and redeploys every game that has mods deployed. It waits while a game is running.">
      <Row title="Automatic updates" desc={on ? <>Checking hourly via {state.autoupdate.scheduler}{game ? <> · {game.name} last ran {ago(game.autoupdate.lastOk)}</> : null}</> : 'Off'}>
        <Toggle checked={on} disabled={busy} label="Automatic updates" onChange={(v) => run(['autoupdate', v ? 'enable' : 'disable'], { label: `Auto-update ${v ? 'on' : 'off'}`, game: null })} />
      </Row>
      <Row title="Follow collection revisions" desc="Move installed collections to their newest revision.">
        <Toggle checked={state.config.autoupdateCollections} disabled={busy} label="Follow collection revisions"
          onChange={(v) => set('autoupdate-collections', String(v), `Collections follow newest revision: ${v}`)} />
      </Row>
      <Row title="Also check on a schedule" desc="An extra check even without system or game updates.">
        <select className="input md:w-44" value={state.config.autoupdateInterval} disabled={busy}
          onChange={(e) => set('autoupdate-interval', e.target.value, 'Auto-update interval')}>
          <option value={0}>Never</option>
          <option value={6}>Every 6 hours</option>
          <option value={24}>Daily</option>
          <option value={168}>Weekly</option>
        </select>
      </Row>
      <Row title="Run it now" desc="Checks every deployed game immediately.">
        <Button size="sm" disabled={busy} onClick={() => run(['autoupdate', 'run', '--force'], { label: 'Run auto-update now', game: null })}>Run now</Button>
      </Row>
    </Section>
  )
}

function Integration({ busy }: { busy: boolean }) {
  const { state, run } = useApp()
  if (!state) return null
  return (
    <Section id="system" title="Integration">
      <Row title="Mod Manager Download links" desc="Lets the browser hand nxm:// links from nexusmods.com to modman. Replaces any other handler.">
        <Toggle checked={state.nxmRegistered} disabled={busy} label="Handle nxm links"
          onChange={(v) => run(['nxm', v ? 'register' : 'unregister'], { label: `${v ? 'Register' : 'Unregister'} nxm handler`, game: null })} />
      </Row>
      <Row title="Application menu" desc="Adds modman to your launcher. On a Steam Deck, add it to Steam from Desktop Mode to open it in Game Mode.">
        <Button size="sm" disabled={busy} onClick={() => run(['ui', 'desktop'], { label: 'Add to application menu', game: null })}>Add shortcut</Button>
      </Row>
      <div className={cx('px-6 py-5 font-mono text-[12px] leading-relaxed text-zinc-500')}>
        modman {state.version} · config {state.configDir} · data {state.dataDir}
      </div>
    </Section>
  )
}
