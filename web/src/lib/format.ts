export function bytes(n?: number | null): string {
  if (!n) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`
}

export function compact(n?: number | null): string {
  if (n == null) return '0'
  return new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(n)
}

const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })
export function ago(epochSeconds?: number | null): string {
  if (!epochSeconds) return 'never'
  const diff = epochSeconds - Date.now() / 1000
  const steps: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, 'second'], [60, 'minute'], [24, 'hour'], [7, 'day'], [4.35, 'week'], [12, 'month'], [Infinity, 'year'],
  ]
  let v = diff
  for (const [size, unit] of steps) {
    if (Math.abs(v) < size) return rtf.format(Math.round(v), unit)
    v /= size
  }
  return ''
}

export function dateTime(epochSeconds?: number | null): string {
  if (!epochSeconds) return '—'
  return new Date(epochSeconds * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`
}

export const cx = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(' ')
