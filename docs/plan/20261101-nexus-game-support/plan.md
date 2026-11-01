# Plan: support every Nexus game through the Nexus extension catalogue

**Status:** draft
**Date:** 2026-11-01
**Scope:** `crates/vessel-core` (definitions, games, nexus client), `apps/desktop` (Games and Add Game views), `vessel` CLI

## 1. Goal

Vessel should let a user add any game that has a Nexus Mods domain and get a working setup without a hand-written preset. Today, 24 games have a TOML preset in `crates/vessel-core/definitions/`, and only Skyrim Special Edition is tested end to end. Games outside that list show as **Unsupported** (no preset) or **Experimental** (preset, untested).

The aim is not to run Vortex's game extensions. Those are JavaScript written against Vortex's own API, and Vessel can't run them. The aim is to reuse the knowledge they encode (install paths, mod types, plugin formats, load-order rules) and turn it into Vessel's declarative definitions.

## 2. Where we are

| Piece | Current state | Source |
|---|---|---|
| Game catalogue | 24 presets, one per file, with `support = "tested" \| "experimental"` | `definitions/*.toml` |
| Unknown games | `support: None`, the user supplies the Nexus domain and settings | `games.rs` `DetectedGame`, `Games.tsx` `SupportMark` |
| Nexus access | REST v1 (`/games/{g}.json`, mods, files, download links), GraphQL v2 (collections, search) | `nexus/client.rs` |
| Downloads | Premium direct links; free accounts via `nxm://` | tech stack §Nexus |
| Layout rules | `target`, `subdir`, `profile`, staging markers (`bethesda`, `cyberpunk`, `stardew`, ...) | `definitions`, `staging.rs` |

The client already has `game(domain)`. That endpoint returns the game's name and domain, so it can be used to check whether a domain exists. It doesn't include install rules.

## 3. What a "supported game" needs

A game is supported when Vessel can do all of these for it:

1. **Identify it:** Nexus domain, Steam appid, and optionally GOG/Epic ids.
2. **Find it:** install folder, Proton prefix if it's a Windows build.
3. **Know where mods go:** target (game root or a subfolder), mod-type rules, and how to tell archive layouts apart.
4. **Know the load order:** plugin file, its format, and the sorting tool, if any (LOOT covers only `bethesda`).
5. **Run installers:** FOMOD, and vendor scripts such as SMAPI, which are handled by the loaders plan.
6. **Make launch changes:** Steam launch options or DLL overrides, if the game needs them.

Vortex's extensions cover items 1–5 in JavaScript. Item 6 is covered by Vessel's loader plugins.

## 4. Approach

Vessel keeps its own format. The catalogue work is done as an offline import and a review step. Nothing runs at mod-install time.

```
Nexus games list ──► catalogue cache ──► "Unsupported" prefill
                          │
Vortex extensions (licence checked)
        │  (offline, reviewed)
        ▼
  import tool ──► draft TOML (generated = true, support = experimental)
                          │
                    human review + test
                          │
                          ▼
            definitions/*.toml (tested or experimental)
```

### 4.1 Catalogue (all Nexus games)

- Add a `nexus::games()` call for the full games list (REST v1 `games.json`; confirm the endpoint and fields before building on it).
- Cache it in the store (new table `nexus_games`: domain, name, id, updated_at). Refresh on a timer and when the user opens Add Game.
- Use it in three places: the Add Game dialog (type-ahead search by name, not just domain), the "Unsupported" flow (prefill the domain and name), and `vessel game add` (accept a name, resolve it to a domain).
- The catalogue is data, not a preset. A game in it isn't supported until it has a definition.

### 4.2 Rule import from Vortex extensions

- Pick a set of Vortex extensions to source from. Start with the games already in `definitions/`, so the output can be checked against existing presets.
- Licence check first. Vortex's licence and each extension's licence must allow reuse before any rule text is copied. Record the result in `docs/plan/20261101-nexus-game-support/licences.md`. If the licence is unclear, re-implement from observed behaviour, not from code.
- Write an import tool (a script under `scripts/`, not part of the app). It reads an extension's game definition (paths, mod types, plugin format) and writes a draft TOML with `generated = true` and `support = "experimental"`.
- Every generated file goes through review. Reviewers check paths against a real install, and check the mod-type rules against a few real mod archives.

### 4.3 Generic fallback for games with no rules

For a game that's in the catalogue but has no imported rules, use the `generic` profile with a user-chosen target. Staging keeps the archive structure and doesn't guess paths. The Add Game dialog shows an explicit warning, and the game is marked **Experimental**. This lets a user try any Nexus game without waiting for rules.

### 4.4 Promotion to tested

A game moves to `support = "tested"` only after an end-to-end test on real hardware: install a mod, deploy, launch, uninstall. Skyrim SE is the reference. The checklist lives next to the feature parity checklist.

## 5. Data model changes

- Definition TOML gets two fields: `source` (for example `vortex:<extension>` or `manual`) and `generated` (bool).
- A `Support` level beyond `tested`/`experimental` isn't needed. "Unsupported" stays as the state of "no definition".
- Store: `nexus_games` table, with a migration in `crates/vessel-core/migrations/`.

## 6. Waves

| Wave | Work | Done when |
|---|---|---|
| **W1: Catalogue** | `nexus::games()`, store table, type-ahead in Add Game, prefill for Unsupported | Any Nexus game can be added by name; no preset required |
| **W2: Generic path** | Generic profile wired to the catalogue, Experimental warning, target picker | An unsupported game deploys mods with the warning shown |
| **W3: Import tool** | Script that turns an extension's rules into draft TOML, with `generated` and `source` fields | Output for 3 known games matches the hand-written presets |
| **W4: Batch import** | Import more extensions, review queue, licence record | At least 10 new experimental presets, each reviewed |
| **W5: Promotion** | End-to-end checklist, tested set grows beyond Skyrim SE | Each promoted game has a recorded test run |

W1 and W2 deliver "support all games" in the sense of letting any game be added. W3–W5 improve how well each game works.

## 7. Risks and constraints

- **Nexus API terms.** Use a registered app key before distributing. Personal keys are fine for development. Never scrape the website. Respect the rate limits in `ratelimit.rs`.
- **Licence.** Vortex extensions may not be reusable as-is. Gate W3 on the licence check in 4.2.
- **Drift.** Vortex extensions change with Vortex releases. Each imported preset records the source version, so changes are visible and reviewable.
- **Wrong rules.** A bad path or mod-type rule can deploy files in the wrong place. Mitigations: the existing backup and journal, a preview before deploy, and `generated` presets that start as experimental.
- **Flatpak and Proton.** Game folders on SD cards and inside Proton prefixes need the existing sandbox handling. Test imported presets there, not just on native installs.
- **Installers.** Vendor scripts run user code. The consent and preview steps in the loader plugin plan apply to any imported installer.

## 8. Open questions

1. Which Vortex extensions should be imported first? My suggestion is the games already in `definitions/`, then the most-downloaded Nexus games.
2. Is it acceptable to ship generated presets as experimental by default, or should every generated preset be reviewed before release?
3. Should the games catalogue refresh automatically, or only when the user asks?
4. Does the free-account `nxm://` flow need changes for games with multiple file types (for example, optional files)?

## 9. Verify before building

- The REST v1 games-list endpoint and its fields.
- Vortex's current extension layout and licence, and each extension's licence.
- Whether any Vortex rules depend on Vortex-only features (for example, its installer scripting) that can't be expressed in TOML. If so, those games stay on the generic path or need a loader plugin.
