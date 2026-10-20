import { memo, useEffect, useId, useState, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { cx } from '../lib/format'

export const spring = { type: 'spring', stiffness: 100, damping: 20 } as const
export const snappy = { type: 'spring', stiffness: 420, damping: 32 } as const
/** Page and list entrances: short enough that switching tabs feels instant. */
export const quick = { duration: 0.14, ease: [0.16, 1, 0.3, 1] } as const

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger'
type Size = 'sm' | 'md' | 'lg'

const variants: Record<Variant, string> = {
  primary: 'bg-accent-400 text-accent-950 hover:bg-accent-300 shadow-[inset_0_1px_0_rgb(255_255_255/0.35),0_10px_30px_-14px_var(--color-accent-500)]',
  secondary: 'bg-white/[0.06] text-zinc-100 hover:bg-white/[0.1] border border-white/[0.06]',
  ghost: 'text-zinc-400 hover:text-zinc-100 hover:bg-white/[0.05]',
  danger: 'bg-rose-500/10 text-rose-300 hover:bg-rose-500/20 border border-rose-400/15',
}
const sizes: Record<Size, string> = {
  sm: 'h-9 px-3 text-[13px] gap-1.5 rounded-xl',
  md: 'h-11 px-4 text-sm gap-2 rounded-xl',
  lg: 'h-14 px-6 text-[15px] gap-2.5 rounded-2xl',
}

interface BtnProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant
  size?: Size
  icon?: ReactNode
}

export function Button({ variant = 'secondary', size = 'md', icon, className, children, ...rest }: BtnProps) {
  return (
    <button
      {...rest}
      className={cx(
        'inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition-[background-color,color,transform,opacity] duration-200',
        'active:scale-[0.98] active:translate-y-[1px] disabled:opacity-40 disabled:active:scale-100',
        variants[variant], sizes[size], className,
      )}
    >
      {icon}
      {children}
    </button>
  )
}

/** Large primary call-to-action. */
export function CtaButton({ className, ...rest }: BtnProps) {
  return (
    <Button
      variant="primary"
      size="lg"
      className={cx('px-7 font-semibold shadow-[inset_0_1px_0_rgb(255_255_255/0.4),0_18px_40px_-18px_var(--color-accent-500)]', className)}
      {...rest}
    />
  )
}

export function IconButton({ label, className, children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      aria-label={label}
      title={label}
      {...rest}
      className={cx(
        'grid size-10 shrink-0 place-items-center rounded-xl text-zinc-400 transition-colors duration-200 hover:bg-white/[0.06] hover:text-zinc-100 active:scale-95 disabled:opacity-40',
        className,
      )}
    >
      {children}
    </button>
  )
}

export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => { e.stopPropagation(); onChange(!checked) }}
      className={cx(
        'relative inline-flex h-7 w-12 shrink-0 items-center rounded-full p-1 transition-colors duration-300',
        checked ? 'bg-accent-500/90' : 'bg-white/[0.08]',
        disabled && 'opacity-40',
      )}
    >
      <motion.span
        layout
        transition={snappy}
        className={cx('size-5 rounded-full shadow-sm', checked ? 'ml-auto bg-white' : 'bg-zinc-400')}
      />
    </button>
  )
}

export function Skeleton({ className }: { className?: string }) {
  return (
    <div className={cx('relative overflow-hidden rounded-xl bg-white/[0.04]', className)}>
      <div className="absolute inset-0 animate-shimmer bg-gradient-to-r from-transparent via-white/[0.05] to-transparent" />
    </div>
  )
}

/** A status dot that breathes while something is live. Isolated so the loop never re-renders a parent. */
export const StatusDot = memo(function StatusDot({ tone = 'accent', live = false }: { tone?: 'accent' | 'amber' | 'rose' | 'zinc'; live?: boolean }) {
  const color = { accent: 'bg-accent-400', amber: 'bg-amber-400', rose: 'bg-rose-400', zinc: 'bg-zinc-500' }[tone]
  return (
    <span className="relative inline-flex size-2.5 shrink-0">
      {live && <span className={cx('absolute inset-0 animate-breathe rounded-full', color)} />}
      <span className={cx('relative inline-flex size-2.5 rounded-full', color)} />
    </span>
  )
})

export function Chip({ children, tone = 'zinc', className }: { children: ReactNode; tone?: 'zinc' | 'accent' | 'amber' | 'rose'; className?: string }) {
  const tones = {
    zinc: 'bg-white/[0.05] text-zinc-400 border-white/[0.06]',
    accent: 'bg-accent-400/10 text-accent-300 border-accent-400/15',
    amber: 'bg-amber-400/10 text-amber-300 border-amber-400/15',
    rose: 'bg-rose-400/10 text-rose-300 border-rose-400/15',
  }
  return (
    <span className={cx('inline-flex h-6 items-center gap-1 rounded-lg border px-2 font-mono text-[11px] whitespace-nowrap', tones[tone], className)}>
      {children}
    </span>
  )
}

export function EmptyState({ icon, title, children, action }: { icon: ReactNode; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="grid gap-5 py-16 md:grid-cols-[auto_1fr] md:items-start md:gap-8 md:py-20">
      <div className="grid size-16 place-items-center rounded-2xl border border-white/[0.06] bg-ink-2 text-zinc-500">{icon}</div>
      <div className="max-w-[52ch]">
        <h3 className="text-xl font-medium tracking-tight text-zinc-100">{title}</h3>
        {children && <div className="mt-2 text-[15px] leading-relaxed text-zinc-400">{children}</div>}
        {action && <div className="mt-6 flex flex-wrap gap-3">{action}</div>}
      </div>
    </div>
  )
}

export function Field({ label, helper, error, children, id }: { label: string; helper?: ReactNode; error?: string | null; children: (id: string) => ReactNode; id?: string }) {
  const auto = useId()
  const fid = id ?? auto
  return (
    <div className="grid gap-2">
      <label htmlFor={fid} className="text-[13px] font-medium text-zinc-300">{label}</label>
      {children(fid)}
      {helper && !error && <p className="text-[12.5px] leading-relaxed text-zinc-500">{helper}</p>}
      {error && <p className="text-[12.5px] text-rose-300">{error}</p>}
    </div>
  )
}

/** Mod artwork with a deterministic monogram fallback (no generic avatar icons). */
export const Thumb = memo(function Thumb({ src, name, className }: { src?: string; name: string; className?: string }) {
  const [failed, setFailed] = useState(false)
  useEffect(() => setFailed(false), [src])
  const hue = [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7)
  const initials = name.replace(/[^A-Za-z0-9 ]/g, ' ').split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?'
  if (src && !failed) {
    return <img src={src} alt="" loading="lazy" onError={() => setFailed(true)} className={cx('object-cover', className)} />
  }
  return (
    <div
      className={cx('grid place-items-center font-mono text-[11px] font-semibold text-white/70', className)}
      style={{ background: `linear-gradient(135deg, oklch(0.36 0.05 ${hue}), oklch(0.24 0.03 ${(hue + 40) % 360}))` }}
    >
      {initials}
    </div>
  )
})

export function ConfirmDialog({
  open, title, body, confirm, tone = 'danger', onConfirm, onClose,
}: { open: boolean; title: string; body: ReactNode; confirm: string; tone?: 'danger' | 'primary'; onConfirm: () => void; onClose: () => void }) {
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 grid place-items-center bg-ink/70 p-4 backdrop-blur-sm"
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
          onClick={onClose}
        >
          <motion.div
            role="alertdialog"
            aria-modal="true"
            initial={{ opacity: 0, y: 16, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.98 }}
            transition={spring}
            onClick={(e) => e.stopPropagation()}
            className="glass w-full max-w-md rounded-3xl p-7"
          >
            <h2 className="text-lg font-medium tracking-tight text-zinc-50">{title}</h2>
            <div className="mt-2 text-[14.5px] leading-relaxed text-zinc-400">{body}</div>
            <div className="mt-7 flex justify-end gap-2">
              <Button variant="ghost" onClick={onClose}>Cancel</Button>
              <Button variant={tone} onClick={() => { onConfirm(); onClose() }} autoFocus>{confirm}</Button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

export function PageHeader({ eyebrow, title, children, actions }: { eyebrow?: string; title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex flex-col gap-5 pb-8 md:flex-row md:items-end md:justify-between">
      <div className="min-w-0">
        {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
        <h1 className="text-3xl font-medium tracking-tighter text-zinc-50 md:text-[40px] md:leading-none">{title}</h1>
        {children && <div className="mt-3 max-w-[65ch] text-[15px] leading-relaxed text-zinc-400">{children}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </header>
  )
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-2xl border border-rose-400/15 bg-rose-500/[0.06] px-4 py-3 text-[14px] text-rose-200">{children}</div>
  )
}
