# DESIGN.md: Vessel desktop UI

The design system for the Tauri desktop app (`apps/desktop`). It writes down the tokens that already exist; it does not define a new brand.

- Token source: `apps/desktop/src/index.css` (`@theme`, `@layer base/components/utilities`).
- Component class strings: `apps/desktop/src/components/ui/styles.ts`.
- Primitives: `apps/desktop/src/components/ui/{controls,display,feedback,layout}/`.
- Stack: Tailwind CSS 4.3 (`@tailwindcss/vite`, no `tailwind.config`), React 19, Phosphor icons, Geist fonts.
- Constraints that shape the system: a strict CSP (no remote origins, no inline `style`), so fonts are bundled and motion is CSS only. Input can come from a mouse, a keyboard or a gamepad.

Values marked **(gap)** are not in code yet. They fill a section this document must cover and are taken from the existing ramps or the blueprint. Add them to `index.css` before using them.

---

## 1. Character

- Dark, quiet tool UI for managing game mods, built for a desk or a couch (gamepad, 800x480 to 1400px+).
- Zinc neutrals with one desaturated emerald accent. Status uses amber (warning) and rose (error) only.
- Flat surfaces: solid fills, hairline borders, no gradients, no glow. A fixed film grain (`body::after`, `--opacity-grain`) adds texture.
- One highlight for every interactive element: a **solid white fill with dark ink**. It shows on hover (pointer) and on focus (keyboard/gamepad), with no outline ring.
- Mono (Geist Mono) for paths, IDs, counts, eyebrows and meta lines. Sans (Geist) for everything else.
- Motion is short and springy (`--ease-spring`), enter-only, and is switched off under reduced motion.

---

## 2. Token architecture (three layers)

```
primitives            ->  semantic                    ->  component
--color-zinc-400          --color-dim (text.muted)        CARD_SHELL, .input, Button TONES
--color-accent-400        --color-ink-2 (bg.surface)      --focus-text (per control)
--radius-card             --radius-control                rounded-card on Dialog
```

| Layer | Where | Examples |
|---|---|---|
| Primitives | Tailwind default palette (`zinc`, `amber`, `rose`) plus `@theme` ramps | `--color-accent-50..950`, `--color-mono-0..7`, `--spacing` (0.25rem) |
| Semantic | `@theme` role names | `--color-ink`, `--color-ink-2`, `--color-ink-3`, `--color-dim`, `--color-hairline`, `--color-line`, `--color-fill-1..3`, `--color-focus*`, `--radius-control`, `--spacing-control`, `--text-body`, `--transition-duration-base` |
| Component | `styles.ts` constants, `@layer components` classes, per-control custom properties | `CARD_SHELL`, `TILE`, `PILL`, `LIST_ROW`, `.glass`, `.input`, `.eyebrow`, `--focus-text` |

Rules:
- Components reference **semantic** tokens only (`bg-ink-2`, `text-dim`, `border-hairline`, `rounded-control`, `min-h-control`).
- A component that needs a value no semantic token covers gets a new semantic token first (see Conventions).
- Current exceptions: status hues (`text-amber-300`, `text-rose-300`) and neutral text steps (`text-zinc-100..400`) are still used as primitives in components. They are mapped to semantic names in section 3. New code uses the semantic aliases `text-fg` (text.primary, zinc-100) and `text-fg-2` (text.secondary, zinc-400); the other steps get aliases when a screen needs them.

---

## 3. Color

### 3.1 Brand ramp (accent, emerald hue 165)

| Step | Token | OKLCH | sRGB approx. | Use |
|---|---|---|---|---|
| 50 | `--color-accent-50` | 0.97 0.03 165 | #E4FCF0 | selected segment text, rare tint text |
| 100 | `--color-accent-100` **(gap)** | 0.94 0.05 165 | n/a | not used |
| 200 | `--color-accent-200` | 0.90 0.08 165 | #ACEFD1 | selected tab text |
| 300 | `--color-accent-300` | 0.84 0.11 165 | #80E2B9 | accent text, success text, badge |
| 400 | `--color-accent-400` | 0.78 0.12 165 | #62D0A4 | **brand.primary**: primary button fill (Play), selected segment fill, progress, tints |
| 500 | `--color-accent-500` | 0.70 0.12 165 | #46B68C | toggle on track |
| 600 | `--color-accent-600` | 0.58 0.11 165 | #258E6A | selected-tab ring under highlight |
| 700 | `--color-accent-700` | 0.50 0.10 165 | #117555 | accent on the white highlight (AA); selected segment rim and text under the highlight |
| 800 | `--color-accent-800` **(gap)** | 0.42 0.08 165 | n/a | not used |
| 900 | `--color-accent-900` **(gap)** | 0.34 0.06 165 | n/a | not used |
| 950 | `--color-accent-950` | 0.24 0.04 165 | #09251A | text on primary fill |

### 3.2 Neutrals

| Token | Value | sRGB | Role |
|---|---|---|---|
| `--color-ink` | oklch(0.155 0.004 286) | #0C0C0E | page, wells, inputs, header |
| `--color-ink-2` | oklch(0.185 0.005 286) | #121215 | cards, pager toolbar |
| `--color-ink-3` | oklch(0.225 0.006 286) | #1B1B1E | focused input, raised |
| `--color-dim` | oklch(0.66 0.012 286) | #919199 | secondary text (AA on ink-3) |
| `--color-fg` | `var(--color-zinc-100)` | #F4F4F5 | text.primary alias: titles, tile names |
| `--color-fg-2` | `var(--color-zinc-400)` | #9F9FA9 | text.secondary alias: summaries, empty-state text (muted ink under the highlight, like `text-dim`) |
| `--color-zinc-50..400` | Tailwind | #FAFAFA..#9F9FA9 | text steps |
| `--color-hairline` | white / 5% | n/a | card borders, dividers |
| `--color-line` | white / 8% | n/a | well, dashed borders |
| `--color-line-control` | oklch(0.53 0.012 286) | n/a | form control edge: `.input` (TextField, SearchField, Select trigger); opaque so it holds ≥3:1 on every surface |
| `--color-line-strong` | white / 10% | n/a | overlay border |
| `--color-fill-1/2/3` | white / 3, 6, 8% | n/a | rest option, default button and track, selected option |
| `--color-fill-hover` | white / 10% | n/a | toggle off track |
| `--color-surface-overlay` | zinc-900 95% | #17171A | `.glass` (dialogs, menus, toasts) |
| `--color-scrim` | ink 70% | n/a | dialog backdrop |
| `--color-selection` | accent-400 35% | n/a | `::selection` |
| `--color-scrollbar` | oklch(0.32 0.006 286) | n/a | thin scrollbars |
| `--color-mono-0..7` | oklch(0.34 0.05 h), h = 20..335 step 45 | n/a | monogram thumbnails |

### 3.3 Semantic tokens

The app ships **dark only**: there is no `prefers-color-scheme` or `data-theme` code. The light column is a **(gap)** mapping onto the same primitives, so a light theme can be added without new colors. Every light text pair below passes AA.

| Semantic | CSS var (convention) | Dark (shipped) | Light **(gap)** |
|---|---|---|---|
| bg.primary | `--bg-primary` | `--color-ink` | zinc-50 |
| bg.surface | `--bg-surface` | `--color-ink-2` | white |
| bg.raised | `--bg-raised` | `--color-ink-3` (focused fields), `--color-surface-overlay` (overlays) | zinc-100 |
| border.default | `--border-default` | `--color-hairline` | zinc-200 |
| border.input | `--border-input` | `--color-line-control` | zinc-500 |
| text.primary | `--text-primary` | zinc-100 = `--color-fg` (titles zinc-50, body zinc-200) | zinc-900 |
| text.secondary | `--text-secondary` | zinc-300 / zinc-400 = `--color-fg-2` | zinc-700 |
| text.muted | `--text-muted` | `--color-dim` | zinc-600 |
| text.placeholder | `--text-placeholder` | `--color-dim` | zinc-500 |
| brand.primary | `--brand-primary` | accent-400 (fill), accent-300 (text) | accent-700 |
| brand.on-primary | `--brand-on-primary` | accent-950 | white |
| status.success | `--status-success` | accent-300 | accent-700 |
| status.warning | `--status-warning` | amber-300 | amber-800 |
| status.error | `--status-error` | rose-300 (text), rose-200 (on tint) | rose-700 |
| status.info | `--status-info` | zinc-300 | zinc-700 |
| focus.ring (highlight fill) | `--color-focus` | zinc-100 | zinc-900 |
| focus.ink | `--color-focus-ink` | zinc-900 | zinc-50 |
| focus.ink-muted | `--color-focus-ink-muted` | zinc-600 | zinc-400 |
| focus.on-brand | `--focus-text` | accent-700 | accent-300 |
| focus.on-danger | `--focus-text` | rose-700 | rose-300 |

Status tints use one formula: fill `{hue}-400/10`, border `{hue}-400/15`, text `{hue}-300`. Stronger borders for toasts: success `/25`, warn `/30`, error `/40`.

### 3.4 Contrast check (WCAG 2.x, computed from the OKLCH values)

Pass (text 4.5:1, large/UI 3:1):

| Pair | Ratio |
|---|---|
| zinc-100 on ink / ink-2 / ink-3 | 17.8 / 16.9 / 15.6 |
| dim on ink / ink-2 / ink-3 / overlay | 6.3 / 6.0 / 5.5 / 5.7 |
| Placeholder `--color-dim` on ink (field rest) / ink-2 / ink-3 (focused field) | 6.27 / 5.98 / 5.49 |
| Field edge `--color-line-control` vs ink / ink-2 / ink-3 / overlay (UI) | 3.69 / 3.52 / 3.23 / 3.35 |
| zinc-400 on ink-2 | 7.1 |
| accent-300 on ink-2 | 11.9 |
| amber-300 on ink-2 | 12.9 |
| rose-300 on ink-2 | 9.7 |
| accent-950 on accent-400 (primary button) | 8.5 |
| rose-200 on rose-500/15 (danger button) | 11.6 |
| Badge text on its tint (accent, amber, rose) | 10.1 / 10.8 / 8.6 |
| Selected tab: accent-200 on accent-400/15 | 11.6 |
| Selected segment fill accent-400 (Play green) vs ink track and unselected segments / vs ink-2 card (UI) | 10.3 / 9.8 |
| Selected segment text accent-950 on accent-400 (as on Play) | 8.5 |
| Selected segment rim and text accent-700 on the highlight fill (UI / text); accent-400 there would be 1.7 | 5.2 / 5.2 |
| Highlight ink zinc-900 / ink-muted zinc-600 on zinc-100 | 16.1 / 7.0 |
| `--focus-text` accent-700 / rose-700 on zinc-100 | 5.2 / 5.5 |
| Highlight fill vs page (UI) | 17.8 |
| Toggle on track vs ink-2 (UI) | 6.2 |
| Light (gap): zinc-900 on zinc-50, zinc-600 on zinc-50, accent-700 on white, amber-800 on white, rose-700 on white | 17.0, 7.4, 5.7, 7.1, 6.1 |

Failures and notes:

| Pair | Ratio | Needs | Note |
|---|---|---|---|
| Toggle off track `--color-fill-hover` vs ink-2 | 1.30 | 3 | Passes in practice: the zinc-400 knob (7.1:1) carries the state. Not moved to `--color-line-control`: the off knob (zinc-400) would drop to about 2.1:1 on it, so the fix needs a knob change too. |
| Selected tab fill accent-400/15 vs ink | 1.3 | 3 | The tint alone does not carry the state. Tabs rely on the accent-200 text and, under the highlight, an accent-600 ring **(gap)**. Segmented no longer uses the tint (solid accent-400, the Play green, 10.3:1). |
| Card border `--color-hairline` vs ink-2 | 1.12 | n/a | Decorative. Cards are not controls. |
| Light (gap) border zinc-200 vs white | 1.27 | n/a | Decorative only. Use zinc-500 (≥4.5) for input edges, not zinc-400 (2.6). |
| Disabled (`opacity 0.45`) zinc-100 text on ink-2 | 4.2 | exempt | Disabled controls are exempt. |

---

## 4. Typography

- Families: `--font-sans: "Geist Variable", ui-sans-serif, system-ui, sans-serif`. `--font-mono: "Geist Mono Variable", ui-monospace, "SFMono-Regular", monospace`.
- Loading: bundled with `@fontsource-variable/geist` and `@fontsource-variable/geist-mono` and imported in `main.tsx`. The CSP blocks remote fonts. No preload step; the fallbacks are system UI fonts.
- Features: `body { font-feature-settings: "ss01", "cv11"; }`, with `antialiased`.
- Weights: 400 (default), 500 `font-medium` (titles, buttons, labels), 600 `font-semibold` (primary button, bold spans). 700 is not used.
- Tracking: `tracking-tight` (titles), `tracking-tighter` (view h1), `--tracking-eyebrow: 0.14em` (uppercase mono eyebrows).

| Token | Size | Use |
|---|---|---|
| `text-micro` | 11px (0.6875rem) | badges, eyebrows, monogram initials |
| `text-caption` | 12px | mono paths, meta, pager count |
| `text-label` | 13px | field labels, helper text, small buttons, muted notes |
| `text-body` | 14px | body, rows, menus, md buttons, tabs |
| `text-lead` | 15px | inputs, lg buttons, view intro |
| `text-title` | 16px | card titles |
| `text-lg` | 18px | sub-heads |
| `text-xl` | 20px | dialog title, empty-state title |
| `text-2xl` | 24px | n/a |
| `text-3xl` | 30px | view h1 below `md` |
| `text-display` | 40px | view h1 at `md` and up (`leading-none`) |

Line heights: the size tokens carry no line height, so text inherits Tailwind's 1.5. Overrides: `leading-none` (display), `leading-tight` (headings), `leading-snug` (muted notes), `leading-relaxed` (1.625: helper, intros, subtitles). Mono logs use 1.7 (see Drift).

Tabular numbers: there is no `tabular-nums` usage yet **(gap)**. Use `tabular-nums` on numbers that change in place or line up in columns: pager counts, sizes, progress, the job island counter.

---

## 5. Space and grid

- Base: 4px (`--spacing: 0.25rem`, Tailwind's default). Every gap, margin and padding is `n × 4px`. Half steps (`*-0.5/1.5/2.5/3.5`, 2px offsets) exist in code; see Drift.
- Named sizes: `--spacing-section` 16px (padding of every card section), `--spacing-rail` 3px (activity bars).
- Layout:
  - Window 1280x800 by default, minimum 1024x640 (`tauri.conf.json`). Dialogs fit down to 800x480.
  - `<main>` content: `max-w-content` (1400px), `px-10 pt-10` (40px).
  - View blocks: `VIEW_FRAME` = `gap-6` (24px).
  - Card header to surface: `mt-2.5`.
- Grids: fixed column counts (`grid-cols-2/3/4`), `minmax(0, …)` splits, and `auto-fill minmax(12rem, 1fr)` for fact lists. There is no 12-column grid.
- Breakpoints: Tailwind defaults `sm 640`, `md 768`, `lg 1024`, `xl 1280`.
- Measures: `--container-prose` 65ch, `--container-prose-sm` 52ch.

---

## 6. Shape and depth

### Radius (scaled from `--radius-card`)

| Token | Value | Use |
|---|---|---|
| `rounded-control-sm` | card × 0.5 = 4.8px | options inside a control, badges, checks |
| `rounded-control` | card × 0.75 = 7.2px | buttons, inputs, thumbs, menus, list rows, nav items, notes nested in a card |
| `rounded-card` | 0.6rem = 9.6px | cards, tiles, dialogs, notices, wells, lg buttons |
| `rounded-card-lg` | card × 1.0 = 9.6px | large media tiles (game posters, collection art) |
| `rounded-pill` | 9999px | dots, switches, progress, counters, tabs, segmented track and segments, job island |

Nest radii: inner radius = outer radius − padding; when that is off the scale, the inner element takes the next step down. Segmented is a `rounded-pill` track (`p-1`) around `rounded-pill` segments: a pill minus the padding is still a pill, so the corners are exactly concentric. A picture inside an activatable card (`CARD_TILE`, `rounded-card-lg` with `p-3`) is `rounded-control-sm`: outer radius minus the padding is below zero, so it takes the smallest step rather than the card's radius. A picture in a dialog body (Browse details) is `rounded-control`, one step below the dialog's `rounded-card`. Components use no raw `rounded-sm..3xl/full`.

### Opacity

| Token | Value | Use |
|---|---|---|
| `opacity-disabled` | 0.45 | disabled and `aria-disabled` controls (base layer) |
| `opacity-stale` | 0.6 | content kept on screen but not actionable: Browse's previous page during the skeleton delay (pair with `inert`), and a result tile that cannot open (no Nexus id; `aria-disabled`). Lighter than `opacity-disabled` so the badge saying why stays readable |
| `--opacity-grain` | 0.035 | film grain (`body::after`) |

### Depth: surface steps (chosen)

Depth comes from surface steps plus a hairline border. Shadows are used only for overlays.

| Level | Surface | Border | Used by |
|---|---|---|---|
| 0 sunken/page | `bg-ink` | `border-hairline` (wells) | page, `TILE`, `.input`, `PILL` |
| 1 surface | `bg-ink-2` | `border-hairline` | `CARD_SHELL`, pager toolbar |
| 2 raised | `bg-ink-3` | n/a | focused input |
| 3 overlay | `.glass` = `bg-surface-overlay` + `backdrop-blur-xl` + `border-line-strong` + `--shadow-overlay` (`0 20px 50px -20px rgb(0 0 0 / .7)`) | `border-line-strong` | Dialog, Select list, Notice, JobIsland |

### z-index scale

| Value | Layer | Where |
|---|---|---|
| 10 | sticky in-flow toolbars | Browse pager (`views/Browse.tsx`) |
| 30 | dropdown/listbox | `Select` list |
| 40 | bottom stack: toasts and job island | `App.tsx` notices layer |
| 50 | modal + scrim | `Dialog` (portal on `<body>`) |
| 60 | film grain (non-interactive) | `body::after` |

Use these values only. The scale is not tokenized yet **(gap)**. Proposed names: `--z-sticky` 10, `--z-dropdown` 30, `--z-toast` 40, `--z-modal` 50, `--z-grain` 60.

---

## 7. Icons

- Library: `@phosphor-icons/react` 2.1.10, used through the `*Icon` exports (e.g. `CaretDownIcon`). Use no other icon set.
- Phosphor glyphs are fills, so there is no stroke width. Weight sets the line: `regular` (default), `bold` (spinners, small glyphs in badges), `duotone` (status and feature icons: Notice, empty states), `fill` (rare).
- Color: `currentColor`, set through `text-*`. Decorative glyphs get `aria-hidden`. Icon-only buttons use `IconButton` (`label` gives `aria-label` and `title`).

| Size | Use |
|---|---|
| 12 | inside badges |
| 14 | small-button glyphs, disclosure carets |
| 16 | default inline glyph, busy spinner, Select caret |
| 20 | view tabs, standalone UI glyphs |
| 28 | empty-state and feature tiles |
| 32 | hero/feature (rare) |

Hit area: icon buttons are `size-control` (48px). Any icon control is at least `min-h-control-sm` (44px).

---

## 8. Motion

- Easing: `--ease-spring: cubic-bezier(0.16, 1, 0.3, 1)` for entrances, presses, toggle knob, tab pill and progress. `ease-out` for fades. `ease-in-out` for loops (`breathe`, `bar`).
- Duration tokens (`duration-*` utilities):

| Scale | Token | Value | Use |
|---|---|---|---|
| 100 | `--transition-duration-micro` **(gap)** | 100ms | entrance fades (`animate-fade` 120ms, `animate-rise` 140ms sit here) |
| 150 | `--transition-duration-fast` | 150ms | option hover, small color changes |
| 200 | `--transition-duration-base` | 200ms | buttons, rows, carets, `animate-pop` |
| 300 | `--transition-duration-slow` | 300ms | toggle track/knob, progress width |
| 400 | `--transition-duration-large` **(gap)** | 400ms | unused |
| 500 | `--transition-duration-xl` **(gap)** | 500ms | unused |

- Animations: `animate-rise` (view and list entrance, 4px), `animate-fade` (scrim), `animate-pop` (dialog, 16px + scale .97), `animate-island` (toasts and job island, 24px, 240ms), `animate-breathe` (live status dot, 2.8s loop), `animate-bar` (job bars, 0.9s loop).
- Stagger: `.stagger` children delay by `--stagger-step` (15ms), capped at 7 steps. Browse's results grid uses it.
- Loading delay: a load replaces what is on screen with skeletons only after 300ms (the `slow` step, `SKELETON_DELAY_MS` in `views/Browse.tsx`). Until then the previous content stays up at `opacity-stale`, `inert`.
- Press: `active:translate-y-px active:scale-98` on buttons, `active:scale-98` on tabs. Activatable cards (`CARD_TILE`) use `translate-y-px scale-99`: at 98 a ~300px card's edges would move ~3px. Transform only, so nothing reflows; under reduced motion the press applies without the transition, like buttons.
- The highlight never animates. `html[data-input] *` limits transitions to transform, translate, scale, rotate and opacity, so the white fill snaps on and off.
- Reduced motion: a global `@media (prefers-reduced-motion: reduce)` rule sets animation and transition durations to 0.01ms. Skeleton pulse uses `motion-safe:animate-pulse`. The tab pill uses `motion-reduce:transition-none`.

---

## 9. Density

Controls are sized for gamepad and touch as well as the mouse.

| Token | Height | Use |
|---|---|---|
| `--spacing-control-xs` | 32px | tab pills (LB/RB also switch tabs) |
| `--spacing-control-sm` | 44px | small buttons, listbox options; the minimum touch target |
| `--spacing-control` | 48px | default buttons, inputs, Select, Toggle, icon buttons (`min-w-control` too) |
| `--spacing-control-lg` | 56px | large buttons |
| `--spacing-thumb` | 44px | 16:9 mod thumbnail height in rows |
| `--spacing-tile` | 64px | empty-state icon tile |
| `--spacing-badge` | 24px | badges |
| `--spacing-icon` | 20px | switch knob, check marks |
| `--spacing-dot` | 10px | status dot |
| Card section / `Row` | 16px padding | settings rows |
| `LIST_ROW` | `px-3 py-2.5` | free-standing rows |
| Toast / island | `min-h-14` (56px) | Notice, JobIsland |

---

## 10. Components

Shared states for every interactive component:
- **Hover** (pointer, `@media (hover: hover)`): the white highlight (`--color-focus` fill, `--color-focus-ink` text). It applies to `button`, `a[href]`, `[role=option]` and `li[tabindex]`, and only to the innermost hovered control.
- **Focus** (keyboard or gamepad, `html[data-input="nav"] :focus`): the same white highlight. The outline stays transparent and is drawn only in forced-colors mode. Text fields show the highlight on nav focus only.
- **Active**: `translate-y-px scale-98` (cards: `scale-99`, see `CARD_TILE`).
- **Disabled**: native `disabled` or `aria-disabled="true"` gives `opacity: var(--opacity-disabled)` (0.45), `cursor: not-allowed` and no highlight.
- **Busy/loading**: `aria-busy` + `aria-disabled`. The control stays focusable and clicks are ignored. A spinner (`CircleNotchIcon` 16 bold `animate-spin`) replaces the leading icon.
- Muted text inside the highlight switches to `--color-focus-ink-muted`. A control can keep its tone with `--focus-text`.

### Button (`controls/Button.tsx`)

| Tone | Rest | Highlight text |
|---|---|---|
| `primary` | `bg-accent-400 text-accent-950 font-semibold` | accent-700 |
| `default` | `bg-fill-2 text-zinc-100 border-hairline` | focus-ink |
| `danger` | `bg-rose-500/15 text-rose-200`, transparent border | rose-700 |
| `ghost` | `text-zinc-300` (no pressed look of its own; Segmented adds its selected style) | focus-ink |

| Size | Height | Padding / text | Radius |
|---|---|---|---|
| `icon` | 48 square | n/a | control |
| `sm` | 44 | `px-3.5 text-label gap-1.5` | control |
| `md` | 48 | `px-4 text-body gap-2` | control |
| `lg` | 56 | `px-7 text-lead gap-2.5` | card |

Props: `busy` (spinner, `aria-busy`), `inactive` (`aria-disabled`, keeps focus; use it instead of `disabled` on any button that focus returns to).

```tsx
<Button tone="primary" icon={<DownloadIcon aria-hidden size={16} />} busy={installing}>Install</Button>
<IconButton label={`Move ${m.name} up`}><ArrowUpIcon aria-hidden size={20} /></IconButton>
```

### Input: TextField, SearchField (`.input`)

- `.input`: `min-h-control rounded-control border-line-control bg-ink px-3.5 text-lead text-zinc-100`, `placeholder:text-dim` (lighter than the zinc-100 text, AA on ink and ink-3), `focus:bg-ink-3`. The edge stays `--color-line-control` under the white highlight (the inset fill paints inside the border).
- Label above in `text-label font-medium text-zinc-300` (`hideLabel` makes it `sr-only`). Helper text in `text-label text-dim`.
- Error: `aria-invalid`, plus `<p class="text-label text-rose-300">` linked through `aria-describedby`.
- SearchField: `role=search` form, `w-96`, `MagnifyingGlassIcon` 18 at `left-3.5`, `pl-10`. Enter submits when `onSubmit` is set; without it the field filters as you type.
  - `pending` (with `onSubmit`): the typed text differs from the submitted one. A trailing `↵ Enter` hint shows (`font-mono text-caption text-dim` at `right-3.5`, input `pr-20`; `aria-hidden`, not a control; hidden in controller mode, where the on-screen keyboard's Search key submits; `focus-ink-muted` on the nav highlight). Browse submits on Enter rather than as you type so each keystroke does not query Nexus.
- States: default; hover (no highlight on fields); focus (`bg-ink-3`, white highlight under nav input); disabled (no dedicated style yet **(gap)**: use `opacity-disabled`); error (as above).

```tsx
<TextField label="Nexus API key" helper="Find it under Site preferences." error={err} />
```

### Select / dropdown (`controls/Select.tsx`)

- The trigger uses the `.input` look with `CaretDownIcon` 16 zinc-400. `inline` puts an eyebrow label on the same row.
- The list is `.glass rounded-control p-1`, `z-30`, `max-h-72`. Options are `min-h-control-sm rounded-control-sm text-body`. The selected option is `bg-fill-3 text-zinc-50` with a `CheckIcon` 15 accent-300 (accent-700 under the highlight).
- Keys: Enter/A opens; arrows, Home and End move; Esc/B closes and returns focus. `busy` shows a spinner and the list does not open.

### Toggle (`controls/Toggle.tsx`)

- `role=switch`, 48px hit box. Track `h-7 w-12 rounded-pill`: on `bg-accent-500/90`, off `bg-fill-hover`. Knob `size-icon`: on white, off zinc-400. Both move with `duration-slow ease-spring`.
- Under the highlight the on track becomes accent-700 and the off track zinc-500, keeping ≥3:1. `busy` uses `aria-disabled`.

### Tabs (`controls/Tabs.tsx`)

- Pills: `min-h-control-xs rounded-pill px-3 xl:px-4 text-body font-medium text-zinc-400`, on one line, scrolling sideways when narrow.
- Selected: `text-accent-200` on a `bg-accent-400/15` pill (scale .95 to 1 plus opacity, 250ms spring).
- Selected + highlighted: accent-700 text and a `ring-2 ring-accent-600` inset ring.
- Keys: roving tabindex with APG manual activation (arrows, Home, End). LB/RB switch tabs.
- With a panel there is a hairline under the bar, and only the panel scrolls.

### Segmented (`controls/Segmented.tsx`)

- An eyebrow label, then a `rounded-pill border-hairline bg-ink p-1 gap-1` track holding `ghost sm` buttons with `aria-pressed` and `rounded-pill` (classes in `SEGMENT`, `Segmented.tsx`). The 4px gap shows between a lit segment and its neighbour.
- Selected: one solid thumb in the Overview Play button's green, `bg-accent-400 text-accent-950` with `--focus-text: accent-700` (the `primary` Button tone's colours; a test keeps the two in step). The fill alone is 10.3:1 against the ink track and the unselected segments, text 8.5:1. Unlike Play it stays `font-medium`, so the label width does not change as the selection moves. It shares the accent with the selected tab (same hue, accent text under the highlight) but is solid rather than a tint: a filter value, not a place.
- A `outline-2 -outline-offset-2 outline-accent-400` rim matches the fill, so it is invisible at rest. Under the highlight (an inset box-shadow, which an outline does not replace; no `ring-*`, which would) the rim turns accent-700 (`highlighted:outline-accent-700`; accent-400 on the white would be 1.7:1) and the text stays accent-700 (`--focus-text`), both 5.2:1, so selected + highlighted differs from highlighted by shape and text, not hue alone.
- States: default (`text-zinc-300` on the track); hover and nav focus on an unselected segment (the white highlight, focus-ink text); selected (accent-400 thumb); selected + hover or focus (white fill, accent-700 rim and text); active (the button press, `translate-y-px scale-98`); disabled/busy: not exposed by the API (the Button styles apply if added). The thumb snaps between segments (colours never animate, see Motion).
- Exception: Browse's Sort is a `Select inline`, not a third Segmented. Show + Created + Sort as segments need about 1060px; the card row has about 910px at the 1024px minimum window, so it would wrap.

### Badge (`display/Badge.tsx`)

- `h-badge rounded-control-sm border px-2 font-mono text-micro`.
- Tones: `zinc` (fill-2), `accent`, `amber`, `rose` (each `{hue}-400/10` fill, `/15` border, `-300` text).
- Not interactive. `SupportBadge` is an amber badge with `FlaskIcon` 12 bold and the text "Experimental".

### StatusDot (`display/StatusDot.tsx`)

- `size-dot rounded-pill`. Tones: accent, amber, rose, zinc-500. `live` adds `animate-breathe`.

### Card, CardSection, Row (`layout/`)

- Shell: `CARD_SHELL` = `rounded-card border-hairline bg-ink-2`. Sections are split by `divide-hairline`, with `p-section` padding.
- Header outside the shell: `CARD_TITLE` (`text-title font-medium tracking-tight text-zinc-50`) plus `MUTED_NOTE` intro plus actions. `inset` puts an eyebrow title inside the shell instead.
- `Row`: title (`text-body text-zinc-100`) and description on the left, controls on the right, wrapping when narrow.
- Activatable card (`CARD_TILE`): the whole `<button>` is one control with `rounded-card-lg`, and it takes the highlight. Padding is per use: Browse tiles `p-3` (vertical tiles `pb-4`), game tiles `p-2.5 pb-3.5` (legacy, see Drift). The text column sits `px-1` in from the picture's edge on both (a 4px optical inset). Browse tiles are named by their title (`aria-labelledby`) and described by byline, stats and status badges (`aria-describedby`), never an `aria-label`.
  - States: rest; hover and nav focus (the white highlight); active/pressed `enabled:not-aria-disabled:active:translate-y-px scale-99` (transform only; the highlight stays on while pressed); inert (Browse's dimmed `<div aria-disabled>` at `opacity-stale`): no highlight and no press. Gamepad A activates by `click()` without `:active`, so there is no press there, as with buttons.

```tsx
<Card title="Deployment" intro="How mods reach the game folder.">
  <Row title="Link method" desc="Hard links need the same drive."><Select … /></Row>
</Card>
```

### Dialog / modal (`dialogs/Dialog.tsx`)

- Portal on `<body>`. Backdrop: `fixed inset-0 z-50 bg-scrim backdrop-blur-sm animate-fade p-4 sm:p-6`.
- Panel: `.glass rounded-card p-5 sm:p-7 animate-pop`, `max-w-2xl` (`wide`: `max-w-5xl`), height `100vh - 2rem/3rem`.
- Title: `text-xl font-medium tracking-tight text-zinc-50`. Subtitle: `text-body text-zinc-400`. Body `mt-6`. Pinned footer `pt-4`.
- Behavior: focus trap, everything else `inert`, Esc/B closes, focus returns to the opener chain.

### Notice / toast (`feedback/Notice.tsx`)

- `.glass rounded-card min-h-14 px-4 py-2 animate-island` in the bottom stack (`z-40`).
- Tones and borders: info `white/10`, success `accent-400/25`, warn `amber-400/30`, error `rose-400/40`. Duotone icon 18.
- Timed close, so there is no dismiss button. `role=alert` for errors, `status` otherwise. One optional action.

### JobIsland (`feedback/JobIsland.tsx`)

- `.glass rounded-pill min-h-14 max-w-xl`. A `size-9` status disc (running: accent tint with 3 `animate-bar` rails; failed: rose; cancelled: `white/8`). Title and detail line, `+n` queued. Status only, not focusable.

### ErrorNote (`feedback/ErrorNote.tsx`)

- `rounded-card border-rose-400/15 bg-rose-500/5 px-4 py-3 text-body text-rose-200`, with optional actions such as "Try again".
- Content: a plain lead line that says what failed ("Couldn’t reach Nexus to list mods."), then the raw error as a secondary line (`mt-1 font-mono text-caption text-rose-300`). The retry goes in `actions` (`Button size="sm"`, `ArrowClockwiseIcon` 16, "Try again", `busy` while refetching).

### Empty (`feedback/Empty.tsx`)

- With a title: a `size-tile` icon tile (`bg-ink-2 text-dim`), `text-xl` title, `text-lead text-zinc-400` text, actions `mt-6`, `max-w-prose-sm`.
- Without a title: one line in a `TILE` well, `text-body text-zinc-400`. It shows no actions: an empty state with a way out (Clear search, Show all time, Go to Games) uses the titled form.

### Pagination toolbar (`views/Browse.tsx`)

- `role=navigation`, sticky to the bottom of `<main>` (`z-10`), edge to edge across the card: `bg-ink-2 border-t border-hairline rounded-b-card px-section py-3`.
- First / Previous / Next / Last are `Button size="sm"` with `CaretDoubleLeft/CaretLeft/CaretRight/CaretDoubleRightIcon` 16; Next and Last sit together at `ml-auto`. At the ends they are `inactive` (focus stays). While loading they are `busy`. With one page there are no buttons, only the count.
- Count: `font-mono text-caption text-dim tabular-nums`, e.g. "Page 3 of 120 · 41–60 of 2,384" or "Page 4 · loading…". Not a live region.
- Result summary: one always-mounted `aria-live=polite` line above the grid (`font-mono text-caption text-dim`), e.g. "2,384 mods" or "Results for “lore” · 30 mods", with a ghost "Clear search" button while a search is active. Visually hidden while the empty state or the error says it instead.
- Paging is by offset for every sort, Newest included (backend contract).

### Skeleton (`browse/Tile.tsx`, `TileSkeleton`)

- Bones: `rounded-control-sm bg-fill-2 motion-safe:animate-pulse` (the picture's radius), laid out like the real tile. `aria-hidden`. Shown only on an uncached fetch, and only after the 300ms loading delay (see Motion).
- Count: as many as the page will hold when the total is known from another page of the same filters (`min(PAGE_SIZE, total − first)`), else a full page.

### Progress (`<progress>`, base layer)

- `h-1.5 rounded-pill`, track `fill-2`, value `accent-400`, width transition `duration-slow ease-spring`.

### Other primitives

- `Thumb`: 16:9 at `h-thumb`, `rounded-control`. Monogram fallback uses `.mono-N` with `text-white/75 text-micro` mono initials.
- `Fact`: eyebrow key plus `font-mono text-label text-zinc-300` value.
- `.eyebrow`: `font-mono text-micro tracking-eyebrow uppercase text-dim`.
- `PILL`: mono log and status chips.
- `LIST_ROW`: free-standing rows.
- `ListToggle`: an `sm` button with a rotating caret, labeled "List (n)".

---

## 11. Conventions

- Token naming:
  - The semantic name is `category.role` (`bg.surface`, `text.muted`). Its CSS variable is `--category-role`.
  - In code, tokens live in Tailwind `@theme` namespaces so utilities are generated: `--color-<role>` → `bg-/text-/border-<role>`, `--radius-<role>` → `rounded-<role>`, `--spacing-<role>` → `h-/min-h-/size-<role>`, `--text-<role>` → `text-<role>`, `--transition-duration-<role>` → `duration-<role>`, `--animate-<name>` → `animate-<name>`.
  - Names describe role (`ink-2`, `hairline`, `control-sm`), not value.
- To add a token:
  1. Check that no existing token fits.
  2. Add it to `@theme` in `apps/desktop/src/index.css`, in the namespace that generates the utility you need.
  3. Add a line to the token list comment at the top of `@theme`.
  4. Add it to this file, in the right section and layer.
  5. For a color, check contrast against every surface it sits on (≥4.5 text, ≥3 UI) and against `--color-focus` if it can appear inside the highlight. Add a `--focus-text` override if it fails there.
- Repeated class strings for a component go in `styles.ts` (UPPER_SNAKE constant with a doc comment) or `@layer components`. They do not get copied between views.
- No inline `style` (CSP). No arbitrary values (`text-[13.5px]`, `duration-[250ms]`) in new code; add a token instead.
- Every interactive element documents and implements default, hover, active, focus, disabled and busy, and works with mouse, keyboard and gamepad.

---

## 12. Copy voice

- Plain and direct, in sentence case: "Install", "Deploy now", "Try again", "Previous", "Next".
- Buttons name the action (verb first). Icon-only buttons give the action and its object as the label ("Move SkyUI up").
- Status lines are short fragments: "no folder set", "Free account", "Signed in".
- Separators: middle dot (`·`). En dash for ranges (`41–60`). Real ellipsis for pending states (`loading…`).
- Numbers use `toLocaleString()`.
- Errors say what failed and offer one action ("Try again"). Warnings name the risk ("Not tested end to end; may need manual fixes").
- Eyebrows and meta are uppercase mono. Body text is never all caps.

---

## Appendix: Drift (off-scale values found in code, not edited)

| Found | Count | Where | Snap to |
|---|---|---|---|
| `text-[13.5px]` | 12 | App.tsx, Notice, JobIsland, Overview, Mods, Collections | `text-body` (14) |
| `text-[12.5px]` | 10 | Activity, GameRepair, Settings, Overview, Mods | `text-label` (13) |
| `text-[11.5px]` | 8 | Activity, Mods, Settings (Tile fixed) | `text-caption` (12) |
| `text-[14.5px]` | 5 | Activity, Mods, Collections | `text-body` (14) |
| `text-[15.5px]` | 1 | Requirements (Tile fixed) | `text-title` (16) |
| `text-[10.5px]` | 2 | App.tsx (version, account tier) | `text-micro` (11) |
| `leading-[1.7]` | 2 | Activity (mono logs) | `leading-relaxed` (1.625) or a `--leading-log` token |
| `duration-[250ms]` | 1 | Tabs highlight pill | `duration-base` (200) or `duration-slow` (300) |
| `ease-[cubic-bezier(0.16,1,0.3,1)]` | 1 | Tabs | `ease-spring` (same curve, tokenized) |
| `duration-150` (raw) | 2 | various (`CARD_TILE`'s `duration-200` fixed) | `duration-fast` |
| `animate-island` 240ms, `animate-rise` 140ms, `animate-fade` 120ms | n/a | `@theme` | 200 / 100 / 100 on the motion scale |
| Icon `size={13}` | 1 | Requirements (BrowseDetails mod tile stats fixed) | 12 |
| Icon `size={15}` | 16 | Select check, Games, dialogs (Browse pager fixed) | 16 |
| Icon `size={17}` / `{19}` | 5 / 2 | various | 16 / 20 |
| Icon `size={18}` | 7 | Notice, SearchField | 20 (or 16 inline) |
| Half-step spacing `*-0.5/1.5/2.5/3.5` | 20 / 21 / 14 / 10 | throughout (e.g. `px-3.5`, `p-2.5`, `mt-1.5`; Games tile `p-2.5 pb-3.5`, Browse tiles moved to `p-3 pb-4` and their text inset to `px-1`) | nearest 4px step; `px-3.5` on `.input` and `sm` buttons is a deliberate legacy match |
| Radii 4.8 / 7.2 / 9.6px | n/a | `--radius-card: 0.6rem` and multiples | 4 / 8 / 8–12 on a 4px grid. `card-lg` equals `card` (×1.0), so the step is redundant. |
| `--spacing-rail: 3px` | n/a | job bars | 4 (or keep as a hairline exception) |
| `border-white/5`, `/6`, `/8`, `/10`, `/15`, `bg-white/8` | 5, 1, 1, 1, 1, 1 | App header, GameRepair, Settings, Notice, JobIsland | `border-hairline` (5%), `border-line` (8%), `border-line-strong` (10%), `bg-fill-3` |
| `text-emerald-300`, `text-sky-300` | 4, 5 | `rich/bbcode.tsx` colour map | `text-accent-300`; sky has no token (BBCode content colour, acceptable) |
| Primitive neutrals in components (`text-zinc-400`, `text-zinc-300`, `text-zinc-100`) | n/a | throughout (Browse tiles moved to `text-fg` / `text-fg-2`) | `text-fg` (zinc-100), `text-fg-2` (zinc-400); zinc-300 has no alias yet |
