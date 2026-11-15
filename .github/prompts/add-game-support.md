# Add full game support for one Nexus game

You are completing Vessel's configuration for exactly one game. Vessel is a
Linux mod manager (Rust core in `crates/vessel-core`). The game to work on,
its definition file and the branch are given to you by the caller as
`DOMAIN`, `NAME`, `DEFINITION`, `BRANCH` and `CHECKER` (the path of
`check_game_support_note.py`). Work on that game only.

The caller (`scripts/run-game-support.sh` or the `/add-game-support`
command) owns git.
Do not run `git commit`, `git push`, `git checkout`, `gh pr` or any other
command that changes branches, history or the remote.

## 0. Tools

Only these shell commands are allowed: `cargo fmt`, `cargo clippy`,
`cargo test`, `git diff`, `git status`, `ls` and `python3 <CHECKER>`.
Make every file change with the Edit or Write tool, never with
`python`, `sed`, heredocs or other scripts. Any other shell command is
denied. A denied command does not mean Bash is unavailable: the allowed
commands above still work, and you must still run the checks in section 4.

## 1. Learn the existing pattern first

Read these before researching, so you know what Vessel can express:

- `crates/vessel-core/src/games/definitions.rs`: the definition schema
  (`GameDefinition`, `PluginsDef`, `Ue4ssDef`, `SharedFileDef`), the
  validation in `parse_definition`, the `BUILTIN` registry and the tests.
- `crates/vessel-core/src/loaders/declarative.rs` (`LoaderDef`, its
  `validate`) and `crates/vessel-core/src/loaders/types.rs` (payload,
  launch changes): the `[[loaders]]` schema.
- `crates/vessel-core/src/requirements.rs`: the known tool keys (`ue4ss`,
  `skse`, `f4se`, `sfse`, `nvse`, `obse`, `bepinex`, `melonloader`,
  `red4ext`, `cet`, `redscript`, `smapi`, ...). A `[[loaders]]` key must be
  one of these.
- `crates/vessel-core/src/games.rs` (`KNOWN_PROFILES`) and
  `crates/vessel-core/src/staging.rs`: what each layout profile does.
- Every file in `crates/vessel-core/definitions/`, especially
  `skyrimspecialedition.toml` (the tested reference) and `palworld.toml`
  (UE4SS + shared `mods.txt`).
- `docs/loaders.md` and `docs/plan/20261101-nexus-game-support/plan.md`
  (section 3, "What a supported game needs").

## 2. Research the game

Use WebSearch and WebFetch. Prefer primary sources, in this order:

1. The game's Nexus Mods pages (`https://www.nexusmods.com/<DOMAIN>`), its
   most-endorsed loader/script-extender mod pages, and the Nexus wiki.
2. The official docs or repository of each loader / script extender /
   plugin framework (GitHub README, releases page, install guide).
3. Community modding wikis for the game (for example UESP, the
   Stardew Modding wiki, PCGamingWiki for paths and Proton notes).
4. The game's Vortex extension, for paths and mod types only. Do not copy
   its code; describe the behaviour in Vessel's own format.

Answer, with a source for every claim:

- Steam appid (and GOG/Epic ids if they exist), whether the Linux install
  runs natively or under Proton.
- Where mods go: deploy root relative to the game folder, which archive
  layouts are common, and any folder that only some mod types use.
- Load order: plugin list file, its format and location, sorting tools.
- Required and common loaders, script extenders and frameworks: name,
  Nexus mod id, what files the archive contains, where they are installed,
  how to tell they are installed, whether they are tied to the game build,
  and any launch option or DLL override needed under Proton.
- Anything Vessel's schema cannot express yet.

If sources disagree, say so and pick the most recent primary source.
Never invent a path, appid or mod id. If you cannot verify a value, leave
it out and list it as an open question.

## 3. Write the configuration

Allowed changes (the runner rejects anything else):

- `DEFINITION` (`crates/vessel-core/definitions/<DOMAIN>.toml`).
- `crates/vessel-core/src/games/definitions.rs`, test module only. Add a
  test for the new config, in the style of
  `palworld_declares_ue4ss_and_mods_txt_only_palworld_does`, and update
  existing tests that the new config legitimately changes (for example that
  test's "only palworld has `[ue4ss]`" loop). Do not touch non-test code.
- `docs/game-support/<DOMAIN>.md` (new): the research note, see section 5.

Rules for the definition:

- Keep the SPDX header and existing comments that are still true.
- Use only fields and values the schema accepts. Pick the profile from
  `KNOWN_PROFILES` that matches the game's layout.
- Add `[[loaders]]` entries for the loaders/script extenders the game
  needs, with `required_paths`, `detect`, `payload` and `launch` that match
  your research. Use `couples_to` when the loader is tied to the game build.
- Set `support` from the Gaps result (section 5):
  - Gaps is `None` (the game is fully supported by config alone):
    `support = "tested"`.
  - Gaps lists one or more Rust changes: keep `support = "experimental"`.
- Note: promotion to `"tested"` here is based on research, not
  hardware testing. The user explicitly overrides plan section 4.4 for this
  workflow. Do not edit the plan doc.
- When you set `"tested"`, update the tests that pin support levels
  (`every_other_preset_is_experimental`) so they still pass, for example by
  listing the tested domains explicitly.
- If promoting to `"tested"` breaks a test in a file you may not change,
  do not claim full support: keep `support = "experimental"`, add a Gaps
  bullet naming the failing test and the file it is in, and explain the
  failure in the note.
- Add this exact line directly below the SPDX header, followed by a
  `# Sources: docs/game-support/<DOMAIN>.md` line:
  `# vessel-game-support: complete`
  Add it whether the result is tested or experimental. The picker uses it
  to skip the game in later runs.

If full support needs Rust changes outside the allowed files (a new
profile, a new staging rule, a new requirements tool key, a new loader
kind), do not write that code. Record each one under "Gaps" in the note.
Only list real Rust changes there; open questions and nice-to-haves go in
their own sections.

## 4. Check your work

You MUST run all of these, from the repository root, before you finish,
and fix what fails. Filtered test runs (for example `games::`) are fine
while you iterate, but they do not replace the full run:

```sh
cargo fmt --all -- --check
cargo clippy -p vessel-core --all-targets --locked -- -D warnings
cargo test -p vessel-core --locked
```

If a check still fails after your fixes, stop and say so in the note under
"Check results". The run must fail rather than ship broken config.

## 5. Research note: `docs/game-support/<DOMAIN>.md`

This file is also the pull request description. Use this structure:

```markdown
# <NAME> (`<DOMAIN>`): game support

## Summary
One paragraph: what changed in the definition and why.

## How mods are installed
Deploy root, layouts, load order, Proton notes.

## Loaders and script extenders
| Name | Nexus id | Installs to | Detect | Build-coupled | Launch change | Source |

## Configuration changes
Bullet list of every field added or changed, each with its source.

## Gaps
None

## Open questions
Values you could not verify.

## Check results
The commands from section 4 and pass/fail for each.

## Manual test
Install a mod, deploy, launch, uninstall (plan section 4.4). Promotion to
tested here is based on research, not hardware testing.

## Sources
Numbered list of every URL used, with the date you read it.
```

### Gaps format (machine-checked)

The body of `## Gaps` (blank lines ignored) must be exactly one of:

- the single line `None`, meaning no Rust changes are needed, and the
  definition has `support = "tested"`; or
- one or more lines that each start with `- `, one needed Rust change per
  line (file or module and what must change), and the definition keeps
  `support = "experimental"`.

Nothing else may appear in that section. Check it with:

```sh
python3 <CHECKER> \
  --definition <DEFINITION> --note docs/game-support/<DOMAIN>.md
```

Finish with a one-paragraph summary in your reply: the game, what changed,
the outcome (full support or partial), the check results and any gaps.
