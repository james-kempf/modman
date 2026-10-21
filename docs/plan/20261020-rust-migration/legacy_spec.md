# Legacy behaviour spec: `legacy/modman` v3.2.0

Task T01 of plan `20261020-rust-migration`. This is the porting reference for the Rust rewrite.
Every fact cites `legacy/modman` line numbers (`L<n>` or `L<a>-<b>`). The source is a single bash file of 5050 lines with embedded Python.
This file records what legacy does. It proposes no design. Open questions are marked **AMBIGUITY** and listed again in §16.

Contents
- §0 Checklist §9 answers (items 1-6)
- §1 Command inventory (global flags, commands, aliases, sub-commands, flags)
- §2 Paths, files and environment variables
- §3 Global config keys and defaults
- §4 Game presets and per-game config
- §5 Steam detection
- §6 Layout profiles (staging normalisation)
- §7 Install pipeline, mod database, mod references
- §8 FOMOD modes and stored choices
- §9 Deploy, backup, manifest, purge, verify, conflicts
- §10 plugins.txt
- §11 Collections
- §12 deps (requirements)
- §13 Nexus API, downloads, rate limit, adult content
- §14 nxm register and nxm handling
- §15 Auto-update state machine
- §16 Behaviours missing from the help text, and ambiguities

---

## 0. Answers to checklist §9 (items 1-6)

### §9.1 `game set` keys and per-game config fields (`cmd_game` L4134-4154)
- `game set` accepts exactly these 9 keys (usage string L4136, case L4141-4152): `folder`, `subdir`, `profile`, `appid`, `method`, `prefix`, `plugins-file`, `paks-dir`, `name`. Any other key fails with `Unknown game setting` (L4151). The command needs the key and at least 2 args (L4136). An empty value is allowed because `$# -ge 2` is checked, not the value.
- Per-key handling:
  | key | variable | validation / transform | line |
  | :--- | :--- | :--- | :--- |
  | folder | G_FOLDER | `~` expanded, trailing `/` removed, warns if the folder does not exist (does not fail) | L4142 |
  | subdir | G_SUBDIR | empty becomes `.` | L4143 |
  | profile | G_PROFILE | must be in KNOWN_PROFILES (L301) | L4144 |
  | appid | G_APPID | none | L4145 |
  | method | G_METHOD | empty, or `auto\|hardlink\|symlink\|copy` | L4146 |
  | prefix | G_PREFIX | `~` expanded | L4147 |
  | plugins-file | G_PLUGINS | `~` expanded | L4148 |
  | paks-dir | G_EXTRA | none | L4149 |
  | name | G_NAME | none | L4150 |
- `folder`, `subdir` and `profile` cannot be changed while a deployment is active. The command fails with "Purge the current deployment first" (L4138-4140).
- Persisted per-game fields (12, written by `save_game` L316-326 to `games/<domain>.cfg` as `KEY=%q` lines): `G_DOMAIN G_NAME G_FOLDER G_SUBDIR G_APPID G_PROFILE G_PLUGINS G_PLUGINS_FMT G_EXTRA G_METHOD G_PREFIX G_NEXUS_ID` (L321). Defaults come from `game_defaults` L303-314: name=domain, subdir `.`, profile `generic`, all other fields empty. When a preset exists, it supplies name/appid/subdir/profile/extra. For bethesda presets with `fmt:folder` extra, it also sets `G_PLUGINS_FMT`.
- `G_PLUGINS_FMT` (`star`/`plain`) and `G_NEXUS_ID` cannot be set through `game set`. The format comes only from the preset (L309-311). The Nexus ID is cached from `GET /games/<domain>.json` (L2818-2826, L4030).
- **AMBIGUITY A1:** For bethesda games, `G_EXTRA` holds `<fmt>:<AppData folder>`, and `game set paks-dir` writes the same variable. Setting paks-dir on a bethesda game therefore overwrites the AppData folder used by `detect_plugins_file` (L438). The checklist row says "plugins format" is a `game set` key, but legacy has no such key.

### §9.2 FOMOD mode semantics and choice storage
See §8. Summary: the mode resolution table is at L1383-1391. Choices are stored per mod in `mods.json` `.mods[].fomod`, in the format `{"type":"fomod","options":[…]}` (L1266, L1275, L1332, L1418-1423).
**AMBIGUITY A2:** The global config key `fomod` (`FOMOD_MODE`, default `auto`, L222) is saved, shown and validated (L3973, L3987, L4001), but no install path ever reads it. `cmd_install` passes `--fomod` or an empty string, and an empty string is treated like `auto` (L1390).

### §9.3 Data used by `deps`
See §12. Requirements come from Nexus GraphQL v2 `mod(modId, gameId){ modRequirements { nexusRequirements { nodes { modId modName gameId externalRequirement url notes } } } }` (L2886, L2899). They are cached in `<game>/cache/requirements.json` with a 7-day TTL (L2884, L2915-2947). Collection-level `externalResources` from `revision.json` are added too (L3160-3171). External tools come from a hard-coded table of 13 entries (L2978-2992), detected by file name.

### §9.4 Does legacy block deploy while the game is running?
**No.** `cmd_deploy` (L1738-1897) and `cmd_purge` (L1899-1930) never check for a running game. The only running-game check is `au_game_running` (L4458-4463), and only `autoupdate_game` calls it (L4556). Auto-update also skips a game while Steam `StateFlags != 4` (L4553). `launch` does not block either. It offers to deploy first (L3937-3940).

### §9.5 Aliases and flags
See §1 (full table).

### §9.6 Behaviours missing from the help text
See §16. They include: rate-limit retry/back-off and early stop (L466-475, L3751-3754); `adult` only affects collection GraphQL queries (L2176, L2681); desktop notifications (L157-160, L4279-4304, L4517); the global flock lock (L182-185); the separate auto-update lock (L4484-4485); hidden commands `ui-state`, `ui-detect`, `ui-thumbs` and bare `nxm://…` (L5038-5041); and the environment variables in §2.

---

## 1. Command inventory

### 1.1 Global options (parsed anywhere on the command line before `--`; `main` L4978-4993)
| Option | Effect | Line |
| :--- | :--- | :--- |
| `-y`, `--yes` | ASSUME_YES=1: `confirm` returns yes, `is_interactive` is false | L4982, L87, L128 |
| `-g <d>`, `--game <d>`, `--game=<d>` | FORCE_GAME (lower-cased, L4993) | L4983-4984 |
| `--no-color` | MODMAN_NO_COLOR=1 | L4985 |
| `--debug` | MODMAN_DEBUG=1 (debug lines on stderr) | L4986, L67 |
| `--` | the rest are positional arguments | L4987 |
- Global options are pulled out from any position, including after the command name, because the loop scans all args (L4980-4991). For example, `modman list -y` works.
- With no command, the default is `help` (L4995).
- `help|--help|-h` prints usage. `version|--version|-V` prints `modman 3.2.0`. Both return before `check_deps` and `load_config` run (L4997-5000). `help` still runs `init_dirs`.
- An unknown command prints `Unknown command`, then usage, and exits 1 (L5042-5046).
- Every other command first runs `init_dirs`, `check_deps`, `load_config` and `run_tmp`, then logs `CMD <cmd> <args>` (args are not logged for `login`) (L5002-5006).

### 1.2 Commands and aliases (dispatch L5008-5047; usage text L4892-4973)
| Command | Aliases | Arguments / flags | Parser lines | Usage line |
| :--- | :--- | :--- | :--- | :--- |
| login | — | none; reads the key from a tty (hidden input) or one line of stdin | L4176-4219 | L4900 |
| logout | — | none | L4221-4225 | L4901 |
| game | games | sub-commands, see 1.3 | L4080-4171 | L4902-4906 |
| config | — | `[key [value]]` | L3980-4014 | L4907 |
| install | add, i | `<target>`; `--file\|-f <id>`; `--fomod ask\|default\|none\|reuse`; `--name <n>`; `--deploy`; passed through to collection: `--optional <v>`, `--revision\|-r <n>`, `--download-only`; any other `-*` is an error | L3323-3360 | L4912-4913 |
| download | dl | `<target>`; `--file\|-f <id>`; other args are passed through to collection install | L3365-3392 | L4914 |
| collection | collections, col | sub-commands, see 1.3 | L2787-2804 | L4930-4935 |
| list | ls | `[text]`; `--enabled\|-e`; `--disabled\|-d`; `--collection\|-c <slug>` | L3397-3440 | L4915 |
| info | show | `<mod>` | L3445-3483 | L4916 |
| enable | — | `<mod...>`; `--all\|-a`; `--collection\|-c <slug>` | L3562-3588 | L4917 |
| disable | — | same as enable | L3562-3588 | L4917 |
| uninstall | remove, rm | `<mod...>`; `--delete-archive\|--purge-archive` | L3611-3630 | L4919 |
| reinstall | — | `<mod>`; `--fomod <v>` (not validated); `--ask` | L3635-3665 | L4920 |
| order | move, mv | `<mod> <spec> [arg]`; spec is one of `first\|bottom\|lowest`, `last\|top\|highest`, `up [n]`, `down [n]`, `before <mod>`, `after <mod>`, `#N\|N` | L3670-3704 | L4918 |
| conflicts | — | `[mod]`; `-f\|--files` | L1994-2044 | L4921 |
| deps | requirements, req | `[mod words…]`; `--refresh\|-r`; `--json`; or `ack\|unack <name>` (must be the first arg) | L3488-3557 | L4922-4923 |
| updates | update, outdated | `-i\|--install`; `--all`; `--refresh` | L3709-3810 | L4924 |
| search | find | `[--json] <text…>`; `--json` is recognised only as the first arg | L3815-3846 | L4925 |
| profile | profiles | `list\|ls` (default), `save <n>`, `load\|use <n>`, `delete\|rm <n>` | L3852-3894 | L4926 |
| deploy | — | `-n\|--dry-run`; `--method <m>` | L1738-1747 | L4939 |
| purge | undeploy | `--legacy` (parsed, has no effect: **AMBIGUITY A3**, L1900-1902) | L1899-1930 | L4940 |
| verify | — | none | L1949-1991 | L4941 |
| status | st | args ignored | L4323-4379 | L4942 |
| launch | play, run | args ignored | L3933-3947 | L4943 |
| clean | — | args ignored | L3899-3928 | L4944 |
| log | logs | `-f\|--follow`; `[N]` (default 40) | L3949-3960 | L4948 |
| nxm | — | `register`, `unregister`, `status` (default), `nxm://…` | L4308-4318 | L4908 |
| autoupdate | auto-update, au | `enable\|on [--scheduler systemd\|cron\|autostart]`, `disable\|off`, `status` (default), `run [--force\|-f] [--dry-run\|-n]` | L4760-4769, L4474-4483, L4606-4614 | L4945-4947 |
| ui | gui, web | `desktop`, or args passed to `server.py` (`--port N`, `--no-open`) | L4864-4887 | L4949-4950 |
| ui-state | — (hidden) | none | L4796 | — |
| ui-detect | — (hidden) | none | L4834 | — |
| ui-thumbs | — (hidden) | none | L4848 | — |
| `nxm://…` | — (hidden) | a bare link is handled like `nxm <link>` | L5041 | — |
| help | --help, -h | — | L4998 | — |
| version | --version, -V | — | L4999 | — |

Notes:
- `install --fomod` accepts `""|ask|default|none|reuse` (L3340). The error message lists only "ask, default or none". **AMBIGUITY A4.**
- `install` routes by target (L3342-3358). An existing path is installed from local disk. A target containing `collections/` goes to `collection_install` with the pass-through flags. Anything `parse_mod_target` accepts (L2829-2846: `nxm://` mod link, `nexusmods.com/[games/]<d>/mods/<id>[?file_id=]`, `<game>:<modid>`, bare digits) is installed from Nexus. Otherwise the command fails.
- `install` ends with `--deploy`, which runs `cmd_deploy`, or `after_change` (L3359).
- `download` of a collection URL runs `collection_install … --download-only` (L3376-3378). A mod download takes no lock.

### 1.3 Sub-commands
`game` (L4080-4171). With no sub-command, the default is `list`.
| Sub | Aliases | Args / flags | Lines |
| :--- | :--- | :--- | :--- |
| list | ls | — | L4083-4096 |
| add | — | `<domain> [path]`; `--subdir <s>`; `--profile <p>` | L4097-4108, flow L4019-4078 |
| use | switch, select | `<domain>` | L4109-4113 |
| remove | rm | `<domain> [--delete-data]` (`--delete-data` is accepted only as the 2nd arg, L4116) | L4114-4124 |
| info | show | `[domain]` | L4125-4133 |
| set | — | `<key> <value>` (§0 §9.1) | L4134-4154 |
| detect | — | — | L4155-4168 |

`collection` (L2787-2804). With no sub-command, the default is `list`.
| Sub | Aliases | Args / flags | Lines |
| :--- | :--- | :--- | :--- |
| install | add, i | `<ref>`; `--optional <all\|none\|ask>` / `--optional=<v>`; `--fomod <collection\|ask\|default>`; `--revision\|-r <N>`; `--deploy`; `--download-only` | L2328-2347 |
| download | dl | same as install, with `--download-only` appended | L2791 |
| list | ls | — | L2653-2670 |
| info | show | `<ref>` | L2672-2707 |
| update | upgrade | `<ref> [install options…]` | L2709-2758 |
| remove | rm, uninstall | `<ref> [--keep-mods]` | L2760-2785 |
| *(other)* | — | if the arg parses as a ref and contains `collections`, it is treated as install | L2796-2802 |

Collection ref formats (L2125-2139): `nxm://<game>/collections/<slug>[/revisions/<n>]`, `…nexusmods.com/games/<game>/collections/<slug>[/revisions/<n>]`, `…nexusmods.com/<game>/collections/<slug>[…]`, or a bare slug matching `^[a-zA-Z0-9]{3,12}$` (the active game is used).

---

## 2. Paths, files, environment

| Item | Path / default | Line |
| :--- | :--- | :--- |
| Config dir | `$MODMAN_CONFIG_DIR` or `~/.config/nexus-mod-manager` | L23 |
| Global config | `<cfg>/config.cfg` (bash `KEY=%q`, chmod 600, written atomically via `.tmp`) | L24, L248-262 |
| Game profiles | `<cfg>/games/<domain>.cfg` | L25, L316-326 |
| Log | `<cfg>/nmm.log`, lines `YYYY-mm-dd HH:MM:SS [LEVEL] msg` with ANSI stripped; levels INFO OK WARN ERROR DEBUG NOTIFY CMD | L26, L56-67 |
| v2 manifest | `<cfg>/deployed.manifest` (`name\|path\|src`) | L27, L1933-1947 |
| nxm queue | `<cfg>/nxm-queue/` (`waiting.pid`, `<game>-<mod>-<file>.nxm`) | L28, L1542 |
| Rate-limit file | `<cfg>/ratelimit`: `"<hourly> <daily> <epoch>"` | L29, L458-463 |
| Lock | `<cfg>/modman.lock` (flock fd 9) | L30, L182-185 |
| Auto-update lock | `<cfg>/autoupdate.lock` (flock fd 7) | L4484 |
| Run tmp | `<cfg>/tmp/run.XXXXXX`, removed on exit | L163-178 |
| Data dir | `$MODMAN_DATA_DIR` or config `DATA_DIR`, default `~/nexus-mods` | L220, L232 |
| Per-game data `GDATA` | `<data>/<domain>/` containing `mods/ downloads/ downloads/.meta/ collections/ backup/ overwrite/ profiles/ cache/ .tmp/ mods.json deployment.tsv deployment.dirs backup.list plugins.managed deployed.stamp plugins.txt.orig deps-ack.json autoupdate.json` | L346-360, L1735, L1881, L2077, L3501, L4465 |

Environment variables read: `MODMAN_CONFIG_DIR` (L23), `MODMAN_DATA_DIR` (L232; run-only override, never saved L230-233), `NEXUS_API_KEY` (L218, L233; run-only override), `NO_COLOR` and `MODMAN_NO_COLOR` (L45), `MODMAN_DEBUG` (L67), `MODMAN_NXM_TIMEOUT` (default 900 s, L1541), `MODMAN_NXM_HEADLESS` (L1543), `TERMINAL` (L4262), `XDG_DATA_HOME` (L4232, L4868), `XDG_CONFIG_HOME` (L4397-4398), `COLUMNS` (L88).
Colour is used only when stdout is a tty, `NO_COLOR` is unset, and `MODMAN_NO_COLOR!=1` (L45).

Required tools (`check_deps` L202-214): `curl jq python3 file flock`, plus one of `7zz|7z|7za`, or `bsdtar` if none of those exist. Optional tools: `unrar`, `unzip`, `xdg-open`, `xdg-mime`, `notify-send`, `update-desktop-database`, `steam`, `systemctl`, `crontab`, `loginctl`. If tools are missing, the command prints a distro install hint (dnf/apt/pacman/zypper, L194-200) and exits 1.

Signals: INT/TERM print "Interrupted." and exit 130 (L180). The EXIT trap removes run tmp, the work tmp, and `waiting.pid` if it holds this process's PID (L173-179).

---

## 3. Global config keys and defaults (`load_config` L217-246, `save_config` L248-262, `cmd_config` L3965-4014)

Keys settable through `modman config` (exactly 9, `config_var` L3965-3978):
| CLI key | Variable | Default | Validation | Line |
| :--- | :--- | :--- | :--- | :--- |
| data-dir | DATA_DIR | `~/nexus-mods` | `~` expanded, trailing `/` stripped, must be absolute; warns that existing data is not moved | L220, L4002-4008 |
| deploy-method | DEPLOY_METHOD | `auto` | `auto\|hardlink\|symlink\|copy` | L221, L3997 |
| jobs | DOWNLOAD_JOBS | `4` | integer 1-16 | L221, L3998 |
| cdn | PREFERRED_CDN | `""` (shown as `auto`) | none | L221, L3985 |
| adult | VIEW_ADULT | `false` | `true\|false` | L222, L3999 |
| auto-deploy | AUTO_DEPLOY | `false` | `true\|false` | L222, L3999 |
| fomod | FOMOD_MODE | `auto` | `auto\|ask\|default` (unused, A2) | L222, L4001 |
| autoupdate-collections | AUTOUPDATE_COLLECTIONS | `true` | `true\|false` | L223, L3999 |
| autoupdate-interval | AUTOUPDATE_INTERVAL | `0` (hours; 0 = only after system/game updates) | non-negative integer | L223, L4000 |

Other persisted keys (not settable through `config`): `NEXUS_API_KEY`, `NEXUS_USER`, `IS_PREMIUM` (`true`/`false`/empty), `GAME_DOMAIN` (the active game) (L253-256).
- `config` with no args prints all 9 keys (L3982-3992). With only a key, it prints the value (L3995). An unknown key fails and lists all keys (L3994).
- v2 keys `GAME_FOLDER`/`STAGING_DIR` are migrated to a game profile (L235-244). The plan marks this as Drop.
- `require_login` fails when NEXUS_API_KEY is empty (L264-266).
- Active-game resolution (`need_game` L365-373): `-g`, then `GAME_DOMAIN`, then the only configured game if there is exactly one, else an error.

---

## 4. Game presets (`game_preset` L269-300) ready for TOML

Format: `domain → name | steam appid | deploy subdir | layout profile | extra`. For bethesda, extra = `<plugins.txt format>:<AppData\Local folder>`. For ue, extra = pak mods folder relative to the game root (L269-271).

| domain | name | appid | subdir | profile | plugins_format | appdata_folder | paks_dir | line |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| cyberpunk2077 | Cyberpunk 2077 | 1091500 | . | cyberpunk | | | | L274 |
| skyrimspecialedition | Skyrim Special Edition | 489830 | Data | bethesda | star | Skyrim Special Edition | | L275 |
| enderalspecialedition | Enderal Special Edition | 976620 | Data | bethesda | star | Enderal Special Edition | | L276 |
| skyrim | Skyrim | 72850 | Data | bethesda | plain | Skyrim | | L277 |
| fallout4 | Fallout 4 | 377160 | Data | bethesda | star | Fallout4 | | L278 |
| starfield | Starfield | 1716740 | Data | bethesda | star | Starfield | | L279 |
| newvegas | Fallout New Vegas | 22380 | Data | bethesda | plain | FalloutNV | | L280 |
| fallout3 | Fallout 3 | 22370 | Data | bethesda | plain | Fallout3 | | L281 |
| oblivion | Oblivion | 22330 | Data | bethesda | plain | Oblivion | | L282 |
| morrowind | Morrowind | 22320 | Data Files | bethesda | (none) | (none) | | L283 |
| stardewvalley | Stardew Valley | 413150 | Mods | stardew | | | | L284 |
| mountandblade2bannerlord | Mount & Blade II: Bannerlord | 261550 | Modules | bannerlord | | | | L285 |
| witcher3 | The Witcher 3: Wild Hunt | 292030 | . | witcher3 | | | | L286 |
| valheim | Valheim | 892970 | . | bepinex | | | | L287 |
| outward | Outward | 794260 | . | bepinex | | | | L288 |
| schedule1 | Schedule I | 3164500 | . | melonloader | | | | L289 |
| palworld | Palworld | 1623730 | . | ue | | | Pal/Content/Paks/~mods | L290 |
| clairobscurexpedition33 | Clair Obscur: Expedition 33 | 1903340 | . | ue | | | Sandfall/Content/Paks/~mods | L291 |
| oblivionremastered | Oblivion Remastered | 2623190 | . | ue | | | OblivionRemastered/Content/Paks/~mods | L292 |
| eldenring | Elden Ring | 1245620 | . | generic | | | | L293 |
| control | Control | 870780 | . | generic | | | | L294 |
| hades2 | Hades II | 1145350 | . | generic | | | | L295 |
| warhammer40000spacemarine2 | Warhammer 40,000: Space Marine 2 | 2183900 | . | generic | | | | L296 |
| monsterhunterwilds | Monster Hunter Wilds | 2246340 | . | generic | | | | L297 |

- Morrowind has an empty extra, so it gets no plugins.txt handling (L309, L435, L2048).
- `KNOWN_PROFILES` = `cyberpunk bethesda stardew bannerlord witcher3 bepinex melonloader ue generic` (L301).
- `game detect` iterates all 24 presets in table order (L4158-4160). The same list appears in `ui-detect` L4836-4838. The `login` first-run detection uses a subset of 13 domains (L4208-4209).
- `game add` (L4019-4078):
  1. The domain must match `^[a-z0-9]+$` (it is lower-cased first).
  2. If logged in, it validates the domain via `GET /games/<d>.json`. If there is no preset, the Nexus name is used and `G_NEXUS_ID` is stored.
  3. With no preset, it asks for profile and subdir when interactive.
  4. The profile must be known.
  5. With no path: it tries Steam detection first (and asks to confirm), then the existing G_FOLDER, then prompts. Non-interactive with no path fails.
  6. The path must be absolute. A path that does not exist only gives a warning.
  7. G_PREFIX is set from the Steam compatdata. For bethesda, G_PLUGINS is set from `detect_plugins_file`.
  8. The game becomes active if there is no active game or it is the same game, otherwise only after confirmation.
- `game remove` refuses while deployed (L4119). It deletes the `.cfg`, with `--delete-data` it runs `rm -rf GDATA`, and it clears GAME_DOMAIN if that game was active.

---

## 5. Steam detection (L388-439, L4444-4455)
- Steam roots that have `steamapps/` (L390-392), deduplicated by realpath: `~/.local/share/Steam`, `~/.steam/steam`, `~/.steam/root`, `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`, `~/snap/steam/common/.local/share/Steam`.
- Libraries are the roots plus every `"path"` line in `steamapps/libraryfolders.vdf` (regex line parse, `\\` turned into `/`, kept only if `<p>/steamapps` exists) (L397-409).
- App dir: the first library with `appmanifest_<appid>.acf` whose `"installdir"` folder exists under `steamapps/common/` (L412-422).
- Prefix: `<lib>/steamapps/compatdata/<appid>/pfx`, first match (L424-431).
- Auto-update reads `"buildid"` and `"StateFlags"` from the acf (L4444-4455).

---

## 6. Layout profiles (`normalize_layout` L866-934 and `layout_*` L654-862)

Common helpers:
- Doc files (`DOC_RE` L651): extensions `txt md pdf jpg jpeg png gif webp bmp url htm html rtf doc docx nfo log`, matched case-insensitively.
- `merge_into src dst` (L689-701): if dst does not exist, move src there. If both are directories, merge with `cp -a --remove-destination` and delete src. Otherwise replace dst.
- `place_all src out skipdocs` (L703-711): moves every entry and skips root-level doc files when `skipdocs=1`.
- `dir_has_top` (L671-684): a folder "looks like the deploy root" when it has a subfolder whose lower-cased name is in the profile's TOPS list, or, for bethesda, a file `*.esp|esm|esl|bsa|ba2`, or, for bepinex, `winhttp.dll` or `doorstop_config.ini`.
- TOPS per profile (L654-664):
  - cyberpunk: `archive bin engine r6 red4ext mods tools`
  - bethesda: `meshes textures scripts interface sound music strings video seq grass lodsettings shadersfx materials mcm skse f4se sfse nvse obse fose dialogueviews calientetools netscriptframework nemesis_engine platform distantlod lsdata fonts vis terrain facegen trees lightplacer actors sky lod shaders source pbrnifpatcher pbrtexturesets parallaxgen dyndolod splashes icons bookart menus characters creatures`
  - bepinex: `bepinex doorstop_libs unstripped_corlib`
  - melonloader: `mods plugins userdata userlibs melonloader`
  - witcher3: `mods dlc bin content`
  - ue: first path component of G_EXTRA (lower-cased), and `engine`
  - others: empty

`normalize_layout <src> <out> <name> <strip> <variants>` (L866-934). It sets `MOD_ROOT` (`game` or empty) and `MOD_VARIANT`.
1. **Wrapper stripping** (only when strip=1 and the profile is not generic, stardew or bannerlord; L873-892). While the folder holds exactly one directory and no non-doc files, descend into that directory, except in these cases:
   - Bethesda: descend if the single dir (not named data/data files) contains a `Data` dir or an `*.exe` (L883-886).
   - Stop if the dir name is in TOPS or is `fomod` (L887).
   - Bethesda: stop if the dir is `data` or `data files` (L888).
   - Witcher3: stop if the dir starts with `mod` or `dlc` (L889).
2. **Variant folders** (only when variants=1, the profile is not generic, stardew or bannerlord, and the src root does not already look like the deploy root; L895-920). Candidates are subfolders where `dir_has_top` is true. With 2 or more candidates, the pick is:
   - `PRESET_VARIANT`, matched by exact name;
   - else an interactive numbered prompt (default 1);
   - else the first candidate in sorted order, with a warning.
   The pick is recorded in `MOD_VARIANT`.
3. **Per-profile placement**:
   - **cyberpunk** (L713-752):
     - If `init.lua` is at the root and it does not look like the deploy root, the whole mod goes to `bin/x64/plugins/cyber_engine_tweaks/mods/<mod>`.
     - Otherwise each dir is handled in this order: a TOPS dir goes to the same path; a dir containing `init.lua` goes to the CET mods folder `<dir>`; a dir containing `*.reds` goes to `r6/scripts/<dir>`; a dir containing `*.archive` or `*.xl` has those files flattened into `archive/pc/mod/`; any other dir goes to the root with the warning "Unrecognised folder".
     - Root files of a structured mod (non-doc) stay at the same path.
     - Loose files: `.archive`/`.xl` go to `archive/pc/mod/`, `.reds` to `r6/scripts/<mod>/`, `.yaml`/`.yml` to `r6/tweaks/<mod>/`, `.lua` to the CET mods folder `<mod>/`, `.dll` to `red4ext/plugins/<mod>/`; any other non-doc file stays at the root.
   - **bethesda** (L754-776):
     - If the root has a `Data`/`Data Files` dir together with other non-doc entries, or the root has an `.exe`/`.dll`, then `MOD_ROOT=game` and everything is placed relative to the game root (docs skipped).
     - Otherwise, if a `Data` dir exists, its contents are placed into the deploy root; else the root contents are.
   - **stardew** (L788-792): if `manifest.json` is at the root, the mod goes to `<mod>/`. Otherwise every folder (depth 2-5, shallowest first) that contains `manifest.json` becomes a top-level folder. Otherwise everything is placed as-is.
   - **bannerlord** (L794-798): same as stardew, with the marker `SubModule.xml`.
   - **bepinex** (L800-819): if it looks like the deploy root, everything is placed as-is. Otherwise the dirs `plugins|config|patchers|core` go to `BepInEx/<dir>`, other dirs to `BepInEx/plugins/<dir>`, `*.cfg` to `BepInEx/config/`, and other non-doc files to `BepInEx/plugins/<mod>/`.
   - **melonloader** (L821-832): if it looks like the deploy root, everything is placed as-is. Otherwise `*.dll` files go to `Mods/`, docs are skipped, and the rest stays at the same path.
   - **ue** (L834-841): if it looks like the deploy root, everything is placed as-is. Otherwise every `*.pak|*.utoc|*.ucas|*.sig` (recursive) is flattened into `<G_EXTRA>/`. If none are found, everything is placed as-is.
   - **witcher3** (L843-862): if a `content/` dir is at the root, the whole mod goes to `mods/<mod>`, with the prefix `mod` added if missing. Otherwise a TOPS dir stays at the same path, a `mod*` dir goes to `mods/`, a `dlc*` dir goes to `dlc/`, any other dir stays at the same path, and non-doc files stay at the same path.
   - **generic / unknown** (L932): `place_all` with docs kept (skipdocs=0).
- `<mod>` = `sanitize_name(name)` (L104-112): every char not in `[A-Za-z0-9._-]` becomes `_`, runs of `_` collapse, leading and trailing `_ . -` are trimmed, the result is cut to 60 chars, and an empty result becomes `mod`.
- When FOMOD ran, normalization is called with strip=0 and variants=0 (L1396). Otherwise it uses 1 and 1 (L1398).

---

## 7. Install pipeline, mod DB, mod references

### Archive extraction (L593-645)
- The kind is detected from the extension (`zip 7z rar tar(.gz|.tgz|.bz2|.tbz2|.xz|.txz|.zst)`), with `file -b` magic as a fallback (L593-610).
- Extractor order:
  - zip/7z: `7z x`, then `bsdtar`, then `unzip` (zip only).
  - rar: `unrar x`, then `bsdtar`, then `7z`.
  - tar: `tar -xf --no-same-owner --no-same-permissions` (L620-633).
- A directory is copied with `cp -a` (bundled collection folders, L615-617).
- After extraction, symlinks, FIFOs, sockets and block/char devices are deleted, and permissions are set with `chmod -R u+rwX` (L642-644). Legacy does not check for `..` or absolute paths; it relies on the extractors.

### `install_archive <archive> <meta-json>` (L1359-1426)
- Meta keys: `name version modId fileId md5 source collections[] choices fomodMode replaces author category updatePolicy variant reinstall id` (L1357-1363).
- Mod id (L1366-1376), in order of precedence:
  - meta `.id` (used by reinstall);
  - else `<sanitize(name)>-<modId>-<fileId>` when modId is set;
  - else `sanitize(name)`, with `-2`, `-3`… added on collision (unless reinstall).
- Work dir: `GDATA/.tmp/install.XXXXXX`, so it is on the same filesystem as the staging dir (L1378).
- FOMOD detection: the shallowest `*/fomod/moduleconfig.xml` up to depth 5, matched case-insensitively (L1336-1341). It is skipped when fomodMode=`none` (L1383).
- No files after normalization fails the install with "No installable files" (L1401-1405).
- If `replaces` names another id, that mod's staging dir is deleted. An existing dest is deleted. The new tree is moved into `mods/<id>` (L1406-1411).
- DB record (L1416-1423): `{id, name, version, modId, fileId, md5, source ("local" default), archive (absolute path), collections, author, category, updatePolicy, root ("game"|""), variant, fomod (choices JSON or null), files (count), size (du -sb), installed (epoch)}`.
- Upsert (`JQ_UPSERT` L575-588):
  - If the same id exists, the record is replaced in place. It keeps `enabled`, and `collections` becomes the union of old and new. If `replaces` is another id, that record is removed.
  - Else, if the `replaces` record exists, it is overwritten in place (same position, keeps enabled, union of collections).
  - Else the record is appended with `enabled:true`, at the end, which is the highest priority.
  - Finally every collection's `mods[]` has `replaces` renamed to the new id.

### Mod DB `mods.json` (L520-542)
- Shape: `{"version":1,"mods":[…],"collections":{slug: {...}}}`. The array order is the priority: last wins (L521).
- Mod references (`JQ_RESOLVE` L544-552, `resolve_mod` L555-572) are matched in tiers. The first non-empty tier wins:
  1. `#N` (1-based position)
  2. exact id
  3. exact name (case-insensitive)
  4. Nexus modId (digits)
  5. substring of the name or id (case-insensitive)
  If the winning tier has more than one match, the command fails and lists `#pos id` for each match.

### Install sources
- Nexus (`install_nexus_mod` L3239-3288):
  1. Gets mod info and the file list. A status other than `published` gives a warning.
  2. File choice (`choose_file` L2849-2875): only categories MAIN, UPDATE, OPTIONAL, MISCELLANEOUS, sorted by category then newest first. The default is the `is_primary` file. If there is a single file or the run is non-interactive, the default is chosen.
  3. If the same modId+fileId is installed, it asks to reinstall (default N). If another file of the same mod is installed, it asks to replace it (default Y). On replace, the old `.fomod` and `.variant` are reused, and fomod becomes `reuse` if it was not given.
  4. Name: `--name`, else the mod name, or `"<mod name> - <file name>"` when the category is not MAIN and the names differ.
  5. Afterwards: `deps_fetch` and a full `deps_report`.
- Local file (`install_local` L3290-3318):
  - Copies the file into `downloads/` with `cp --reflink=auto`.
  - Computes md5. If logged in, it calls `GET /games/<d>/mods/md5_search/<md5>.json` to identify the mod (modId, fileId, version, name). If no match is found, the name falls back to the file name.
- `reinstall` (L3635-3665): if the archive is missing and the mod is a Nexus mod, it is downloaded again. The meta is the old record plus `{fomodMode, choices: .fomod, reinstall:true}`. `--ask` also clears the variant.
- `uninstall_ids` (L3593-3609):
  - Deletes the staging dir.
  - With `--delete-archive` (keep=0), it also deletes the archive, but only if the archive is under `downloads/` and no other mod uses it, plus the matching `.meta` files.
  - Removes the ids from mods[] and from every collection's mods[].
  - `cmd_uninstall` asks for confirmation (default Y).
- `order` (L3670-3704): position 0 is the lowest priority. `first`/`bottom`/`lowest` = 0. `last`/`top`/`highest` = n-1. `up n` = cur+n. `down n` = cur-n. `before X` = index of X with self removed; `after X` = that index + 1. `#N` = N-1. The result is clamped.
- `after_change` (L2809-2816): if `AUTO_DEPLOY=true` and the game folder exists, it runs deploy. Otherwise, if a deployment is active, it prints a hint.
- `profile` (L3852-3894):
  - File: `profiles/<name>.json` = `{saved, mods:[{id, enabled}]}`. Names match `^[A-Za-z0-9._-]+$`.
  - `load` reorders: profile mods in profile order with the profile's enabled state, then installed mods missing from the profile, appended disabled. Profile entries that are no longer installed are dropped and counted.
- `clean` (L3899-3928): files in `downloads/` (top level) that no mod record references are candidates. It deletes `.tmp/*` and `<cfg>/tmp/run.*`, asks (default N), deletes the candidates, and prunes `.meta` files whose archive is gone.
- `updates` (L3709-3810):
  1. Calls `GET /games/<d>/mods/updated.json?period=1m`.
  2. Per-mod cache `cache/files-<modId>.json`. The cache is reused if younger than 6 h (21600 s). It is only touched (not refetched) if younger than 28 d and the mod is not in the updated list, or its update is older than the cache.
  3. Stops early when the hourly quota is below 20.
  4. The newest file is the end of the `file_updates` chain (at most 100 hops), restricted to files not ARCHIVED/DELETED.
  5. Mods with `updatePolicy == "exact"` are shown as "pinned by collection" and are skipped by `--install` unless `--all`.
  6. Install uses meta with `fomodMode:"reuse"`, `choices: .fomod`, `replaces: <id>`.
- `launch` (L3933-3947):
  - Needs an appid.
  - If mods changed (mods.json newer than deployed.stamp), it offers to deploy (default Y) and then releases the lock (`exec 9>&-`).
  - Runs `steam steam://rungameid/<appid>` (setsid), or `xdg-open`.

---

## 8. FOMOD modes and choice storage

### Mode resolution (`install_archive` L1383-1391)
`fmode` comes from meta `.fomodMode`. "interactive" means not `-y` and stdin is a tty (L87).
| fmode | choices present | interactive | effective mode |
| :--- | :--- | :--- | :--- |
| `none` | – | – | FOMOD skipped; normal layout (L1383) |
| `collection` | yes | – | `choices` |
| `collection` | no | – | `default` |
| `reuse` | yes | – | `choices` |
| `reuse` | no | yes | `ask` |
| `reuse` | no | no | `default` |
| `ask` | – | yes / no | `ask` / `default` |
| `default` | – | – | `default` |
| anything else (`""`, `auto`) | – | yes / no | `ask` / `default` (L1390) |

Inside the Python installer (L1112-1128), more fallbacks apply:
- `choices` with no usable `options` falls back to `default`.
- `ask` without a usable `/dev/tty` falls back to `default`.

Where each fmode comes from:
| Caller | fmode | choices | Line |
| :--- | :--- | :--- | :--- |
| `install` (Nexus/local) | `--fomod` value or `""` | null; when replacing an older file, the old `.fomod` is used and fmode defaults to `reuse` | L3324, L3267-3271 |
| `collection install` | `--fomod` (default `collection`; allowed: collection, ask, default) | `collection.json` `mods[i].choices` | L2329, L2347, L2491-2496 |
| `reinstall` | `reuse` (default), `--ask` gives `ask`, `--fomod X` | record `.fomod` | L3636-3641, L3659-3660 |
| `updates --install` | `reuse` | record `.fomod` | L3800-3802 |

### Installer behaviour (embedded Python `fomod_py` L941-1333; called with `python3 -I` L1347)
- Args: `root out mode choices.json deploy_dir modname`. `deploy_dir` = `G_FOLDER/<subdir>/` and is used for `fileDependency` checks against the live game folder (L1347, L1006-1010).
- XML: namespaces are stripped. If the parse fails, it retries decoding as utf-16, utf-8-sig, cp1252 and latin-1 (L957-973). Path lookup is case-insensitive with `\`/`/` normalisation, and `..` is refused (L976-996).
- Conditions (L1012-1031): `flagDependency`, `fileDependency` (`Active`/`Missing`; any other state is false), and nested composites with the `And`/`Or` operator. Game, script-extender and fomm version checks always count as true.
- Plugin type comes from `typeDescriptor/type`, or `dependencyType` patterns, then `defaultType`, else `Optional` (L1033-1049).
- Sort order (L1051-1059): `order` attribute Explicit, Ascending (default) or Descending, by lower-cased name.
- Defaults per group type (L1061-1074):
  - SelectAll = all.
  - SelectExactlyOne = the first of Required, else Recommended, else usable, else all.
  - SelectAtMostOne = the first Required/Recommended, or none.
  - SelectAtLeastOne = Required plus Recommended, else the first usable.
  - SelectAny = Required plus Recommended.
- `choices` mode (`from_preset` L1088-1110):
  - The step is matched by name (exact, then case-insensitive), then the group.
  - Each choice is matched by name. If the name is duplicated, `idx` is used. If no name matches, `idx` is used when in range.
  - Required plugins are always added, NotUsable are removed, and the group-type constraints are enforced (falling back to defaults).
  - A group with no preset entry uses the defaults (L1262-1265).
- `ask` mode (L1162-1211): numbered list with markers for defaults, type labels and descriptions. Input is a number, a range `1,3-4`, `a`/`all`, or `0`/`n`/`none`. `?N` shows details and Enter takes the default. Required plugins are forced on and NotUsable are rejected. Exactly-one, at-most-one and at-least-one are enforced.
- File collection (L1219-1283):
  - `requiredInstallFiles`.
  - For each visible step (`visible` condition) and each group: the files of selected plugins (their condition flags are set), plus files of unselected plugins marked `alwaysInstall`, or `installIfUsable` when the plugin is not NotUsable.
  - Then `conditionalFileInstalls` patterns.
  - The list is sorted by `(priority, sequence)`, so a later or higher-priority entry overwrites.
- Placement (L1286-1330): hardlink, with copy as a fallback. Folder destinations are safe-relative (unsafe ones are refused and logged). A file with an empty destination gets its basename. A destination ending in `/` gets the basename appended. Missing sources are logged and skipped, and the install continues.
- Output (stdout, L1332): `{"type":"fomod","options":[{"name":<step>,"groups":[{"name":<group>,"choices":[{"name":<plugin>,"idx":<int>}]}]}]}`. This is the Vortex/collection choices format.

### Where choices persist
- `mods.json` `.mods[].fomod` holds the JSON above, or `null` when no FOMOD ran (L1348, L1418-1423).
- `info` prints `FOMOD: <step>: <choice names>` (L3467).
- Collections supply choices from `collection.json` `.mods[].choices` (L2495). Vortex collection choices use the same shape, so they are passed through unchanged.
- The variant pick (not FOMOD) is stored in `.variant` and reused as `PRESET_VARIANT` (L1382, L3269, L3802).

---

## 9. Deploy, backup, manifest, purge, verify, conflicts

### Method resolution (`resolve_method` L1574-1584, `cmd_deploy` L1754-1756)
1. `deploy --method <m>` overrides everything.
2. Otherwise the game `G_METHOD` is used, else global `DEPLOY_METHOD`, else `auto`.
3. `auto` is resolved by a probe: write `mods/.modman-probe.<pid>`, then try `ln` to `<G_FOLDER>/.modman-probe.<pid>`. Success means `hardlink`; failure means `copy`. Both probe files are removed. `auto` never picks symlink.
4. The final method must be `hardlink|symlink|copy`, or the command fails.
- Operations (L1644-1659): hardlink is `ln`, symlink is `ln -s` (absolute source path), copy is `cp --reflink=auto -p`.

### Pre-checks
- `need_game`, then `require_game_folder` (set and exists, L375-378), then the global lock, then the game folder must be writable (L1748-1751).
- There is **no** running-game check (§9.4).

### File plan (`build_plan` L1586-1608)
- Enabled mods are processed in DB order. A mod whose staging dir is missing is skipped with a warning.
- Each staged file has rel = `<prefix><path>`. The prefix is empty for `root=="game"`, else `deploy_base_rel` (the subdir with a trailing `/`, empty for `.`, L380-385).
- The key is the lower-cased rel. The last mod wins `P_OWNER`. `P_REL` keeps the first provider's spelling (L1599-1603). `P_PROV` lists all providers in order. Each entry also records inode, size and mtime (`%Ts`, integer seconds).

### Deploy algorithm (`cmd_deploy` L1758-1897)
1. **No files planned**: if a deployment is active and this is not a dry run, run `undeploy`. Otherwise print "nothing to deploy" (L1763-1772).
2. **Dry run** (L1779-1794): count planned keys that match an existing game file not in the current manifest. That count is "would be backed up". Nothing changes. `deps_check_enabled` runs.
3. **Existing deployment**: if one is active, run a full `undeploy` first (L1796-1799). Redeploy is purge plus deploy.
4. `scan_game_folder` (L1611-1631): index the top-level entries, and recursively only the top-level dirs the plan touches. The maps go from lower-case to real spelling.
5. For each planned key (L1809-1826):
   - Target dirs are resolved case-insensitively. Missing dirs are created with the plan's spelling and recorded in `NEW_DIRS` (`resolve_dir` L1634-1642).
   - If a game file already exists with that key, the target takes the existing real spelling and is added to `backups`.
   - Files whose source basename equals the target basename are batched per dir; others are placed one at a time.
   - The manifest line is written **before** placement.
6. Create the new dirs and append them to `deployment.dirs` (L1829-1832).
7. **Backups** (L1835-1847): for each overridden original:
   - If `backup/<t>` already exists, the current game file is moved to `overwrite/<t>`.
   - Else it is moved to `backup/<t>` and `<t>` is appended to `backup.list`.
8. **Placement** (L1850-1879): `ln`/`cp` runs per dir in chunks of 400. If a chunk fails, every file in it that is not yet present is copied (`cp --reflink=auto -p`), and an extra manifest line is appended with method `copy`, an empty inode, and the source's size and mtime (L1858-1869). Remaining failures are counted. Single files use `link_one`.
9. Write the epoch to `deployed.stamp` (L1881). Run `plugins_update` (L1883). Print the summary (placed, backed up, failed). The first 20 error lines go to the log. Then `deps_check_enabled` (L1896).

### Files
| File | Format | Line |
| :--- | :--- | :--- |
| `deployment.tsv` | one line per placed file, fields separated by **`\x1f`** (not tab): `target_rel_real_spelling ␟ mod_id ␟ method ␟ inode ␟ size ␟ mtime_epoch ␟ src_abs_path` | L1661, L1825, L1864 |
| `deployment.dirs` | newline list of dirs modman created (relative, real spelling); append-only until undeploy truncates it | L1830 |
| `backup.list` | newline list of relative paths whose originals are in `backup/` | L1844 |
| `backup/<rel>` | moved original game files | L1843 |
| `overwrite/<rel>` | files changed after deploy, plus originals displaced when a backup already existed | L1699-1700, L1839-1840 |
| `overwrite/.stale-backups/<rel>` | old backups retired when the game replaced a deployed file | L1694-1695 |
| `deployed.stamp` | epoch of the last deploy | L1881 |
`deployment_active` = `deployment.tsv` or `backup.list` is non-empty (L1663).
**AMBIGUITY A5:** After a chunk falls back to copy, the target has two manifest lines (the original method, then `copy`). The reader keeps the last line (associative array, L1673-1676). Files that failed to place still have manifest lines, so purge counts them as "missing".

### Purge / undeploy (`undeploy` L1666-1736; `cmd_purge` L1899-1930)
`cmd_purge`:
1. Needs the game and folder, then takes the lock.
2. If there is a v2 `deployed.manifest`, it runs `purge_legacy` (L1933-1947). That removes the listed dirs under G_FOLDER after confirmation, then `rmdir <game>/mods`.
3. If nothing is deployed, it says so and returns.
4. It asks for confirmation (default Y) and runs `undeploy`.
5. The summary shows Removed, Restored, Missing, Changed and Kept.

`undeploy` order:
1. **Classify each manifest entry** by `find -printf %y %i %s %Ts %l` (L1679-1704). An entry is unchanged when:
   - hardlink: type `f` and the inode equals the manifest inode;
   - copy: type `f`, and size and mtime are equal;
   - symlink: type `l` and the link target equals the manifest source.
   Unchanged entries are deleted. A changed entry that has `backup/<t>` means the game replaced the file: the game's version is kept, and the old backup moves to `overwrite/.stale-backups/<t>` (counted as changed). A changed entry with no backup is moved into `overwrite/<t>` (changed). Manifest entries that are not present are counted as missing.
2. **Restore originals** from `backup.list`, in list order (L1713-1724). If `backup/<t>` exists and the target is free, move it back (restored). If the target is occupied, keep the backup in the list (kept). `backup.list` is rewritten with only the kept entries.
3. **Remove created dirs** (L1727-1732): `sort -ru` (deepest first, reverse lexicographic), then `rmdir --ignore-fail-on-non-empty`. `deployment.dirs` is truncated.
4. Truncate `deployment.tsv` (L1733).
5. `plugins_remove_managed` (L1734).
6. Delete `deployed.stamp` (L1735).

### verify (L1949-1991)
- Uses the same per-method checks as undeploy. Symlinks must also resolve (`-e`, L1968).
- Prints intact, changed and missing counts, plus up to 40 problems.
- Exits 1 if there are problems.
- Warns when `mods.json` is newer than `deployed.stamp`.
- Takes no lock.

### conflicts (L1994-2044)
- Built from `build_plan`. For each multi-provider key, pairs of loser and final winner are counted.
- An optional mod filter keeps pairs where either side is that mod. `--files` lists up to 25 files per pair.
- Output is sorted by the winner's position.
- `info` shows wins and losses for one mod (L3468-3477).

### "Changed since deploy" checks
`mods.json -nt deployed.stamp` (L1987, L3436, L3937, L4364).

---

## 10. plugins.txt (L2046-2120)
- Applies only when the profile is `bethesda` and `G_PLUGINS_FMT` is non-empty (L2048).
- Path: `G_PLUGINS` if set, else `<prefix>/drive_c/users/steamuser/AppData/Local/<AppData folder>/plugins.txt` (L434-439, L2049-2050). The prefix is `G_PREFIX`, else the Steam compatdata.
- Formats:
  - `star`: active plugins are written as `*Name.esp` (SSE, Enderal SE, FO4, Starfield).
  - `plain`: the bare name (Skyrim LE, FNV, FO3, Oblivion) (L2095).
- `plugins_update` runs after every deploy (L1883):
  1. If the plugins.txt folder does not exist, it warns "Launch the game once" and returns (L2056-2059).
  2. Collects `*.esm|*.esl|*.esp` at the top level of each enabled mod's staging dir, in priority order. For `root=game` mods, it looks in the subfolder matching the deploy subdir (case-insensitive). Names are deduplicated case-insensitively (L2064-2074). Without plugins it returns.
  3. On the first write ever, the original is backed up to `GDATA/plugins.txt.orig`, which is never overwritten (L2077).
  4. Existing non-empty, non-`#` lines are kept verbatim in order (CR stripped). Any `*` prefix is ignored when testing presence (L2081-2087).
  5. Mod plugins not already present are appended: all `.esm`/`.esl` first, then `.esp`, each group in mod priority order (L2088-2089).
  6. The file is written with two fixed `#` header lines and **CRLF** line endings, atomically via `.modman.tmp` (L2090-2098).
  7. The appended names are written to `plugins.managed` (only if some were added) (L2099).
- `plugins_remove_managed` (runs during undeploy, L2103-2120): removes every line whose name (`*` stripped, case-insensitive) is in `plugins.managed`. Other lines are kept, including comments, and written with CRLF. Then `plugins.managed` is truncated.
- Legacy does no LOOT sorting. It prints a hint to use LOOT (L2100). Entries the user already had (for example disabled ones without `*`) are left unchanged.

---

## 11. Collections

### `collection install` (L2328-2561)
1. Parse the flags and ref. `--revision` overrides the ref's revision. Requires login. `use_game_for <domain>` offers to set up the game if it is missing (interactive) or fails (L2150-2161). Takes the lock. Refreshes the account tier if unknown.
2. `fetch_collection` (L2174-2226):
   - GraphQL `collectionRevision(slug, domainName, revision, viewAdultContent: VIEW_ADULT)` (L2163-2170).
   - Stores `collections/<slug>/rev<N>/revision.json`.
   - If `collection.json` is missing, it follows `downloadLink` (an API-relative path, with `download_links[0].URI`), downloads `collection.archive`, extracts it to `content/`, and copies the first `collection.json` found (depth 2 or less).
   - If that fails, it builds a minimal `collection.json` from the API `modFiles` (no choices, no rules), with a warning.
3. Mod table from `collection.json` `.mods[]` (L2358-2365): name (or `logicalFilename`), version, optional, `source.type` (default nexus), modId, fileId, md5 (lower-cased), fileSize. Prints the curator's `installInstructions`.
4. **Optional mods** (L2385-2410): `--optional` (default `ask`, which becomes `none` when non-interactive). `ask` takes a selection like `all`, `none`, or `1,3-5` (`parse_selection` L2229-2247).
5. **Already installed** (L2413-2431): matched by md5 first, else by modId-fileId against the DB. Those are "present" (HAVE). Nexus items with an existing download (`downloads/.meta/<mod>-<file>.json`) are not downloaded again.
6. Disk check: warns if 2 × download bytes exceeds the free space (L2437-2440). Free account: warns about one click per file (L2441-2444). Asks to proceed (default Y).
7. **Acquire archives** by source type (L2452-2463):
   - `nexus`: download queue.
   - `bundle`: find under `content/bundled/` by `fileExpression*`, `logicalFilename*` or `name*` (case-insensitive) (L2612-2623).
   - `direct`: `fetch_file(source.url)` (L2625-2635).
   - `browse|manual`: interactive only. Shows the instructions, opens the URL, and asks for a local path (L2637-2651).
   - Other types fail.
   Downloads (`download_queue` L2576-2610): Premium runs `DOWNLOAD_JOBS` in parallel (background subshells). Free runs them in sequence (each one waits for an nxm click).
8. `--download-only` stops here (L2469-2475).
9. **Install**: HAVE mods get the slug added to `.collections`. Each todo mod with an archive is installed with fomodMode=`--fomod` value and choices from the collection (L2478-2502). Failures are collected and do not abort.
10. **Load order** (L2505-2522): `collection_order_py` (L2249-2315):
    - Index of present mods by `source.tag`, `md5`, `logicalFilename` and `fileExpression` (all lower-cased except tag).
    - Each `modRules` entry of type `before`/`after` is resolved against the index via `tag`, `fileMD5`, `logicalFileName` or `fileExpression`, and becomes an edge.
    - Kahn topological sort. Ties are broken by `(mod.phase or 0, collection index)`.
    - Mods in a cycle are appended in rank order, with a message on stderr.
    - Other rule types (`requires`, `conflicts`, …) are ignored.
    - The resulting ids (deduplicated) are moved into one contiguous block at the lowest current position of any of them (`JQ_REORDER_BLOCK` L2318-2324). If Python fails, the install order is kept.
11. **Record** `.collections[slug] = {slug, name, author, domain, revision, installed, mods:[ordered ids], failed:[names], dir}` (L2527-2533). Prints the summary. If anything failed, re-running skips the finished mods.
12. **Requirements** of the collection's mods (problems mode), including collection `externalResources` (L2548-2555).
13. Deploys if `--deploy` or `AUTO_DEPLOY` (L2556-2560).

### Other collection commands
- `list` (L2653-2670): per collection: installed and enabled counts, failed count, install date.
- `info` (L2672-2707): queries GraphQL (with VIEW_ADULT). Prints the details and whether an update is available versus the local record.
- `update` (L2709-2758):
  1. Queries the latest revision with `viewAdultContent:false` (**AMBIGUITY A6**: it ignores the `adult` setting, L2718).
  2. "Up to date" means the revision is equal and `failed` is empty.
  3. Otherwise it runs `collection_install nxm://<d>/collections/<slug> --revision <latest> [opts]`.
  4. Old ids that are not in the new record: mods that belong only to this collection are "dropped", and the user is asked to uninstall them (default Y). If declined, the slug is just removed from them. Mods shared with other collections only lose the slug.
- `remove` (L2760-2785): "exclusive" mods (`collections == [slug]`) are uninstalled unless `--keep-mods`. Shared mods stay. The slug is removed from all mods and the record is deleted. Then `after_change`.
- A mod record's `collections[]` is the membership used by `list -c`, `enable/disable -c`, removal and deps scope.

---

## 12. deps (requirements)

### Data sources
- Per-game cache `cache/requirements.json` = `{ "<modId>": {t, req:[nodes], avail:bool, ta} }` (L2879-2882).
  - `req` holds the nodes of the GraphQL `modRequirements.nexusRequirements` (L2886).
  - `avail` records whether that modId still resolves on Nexus.
  - TTL is 7 days (`DEPS_TTL=604800`, L2884). `--refresh` ignores the TTL.
- `deps_fetch` (L2915-2947): needs an API key and the game's Nexus numeric id.
  - Stale requirement entries are fetched in batches of 40 aliases (`gql_mod_batch` L2891-2911). When Nexus returns errors for some aliases, those are stored as `null` (marking them unavailable) and the rest are retried, up to 3 tries.
  - Then it checks availability (`modId` only) of required same-game mods. These are either direct requirements, or found through a `nexusmods.com/<domain>/mods/<id>` URL.
- Collection-level requirements: `revision.json` `.externalResources[] {name, resourceUrl}`, labelled `collection: <name>`. They are added for every installed collection with at least one mod in scope (L3160-3171).
- Scope: specific mod ids, or all enabled mods (L2968-2969).

### Analysis (`deps_py` L2949-3155)
- **External-tool list** (`TOOLS` L2978-2992). Each tool is matched by a regex over `modName + url` (case-insensitive) and detected by these file names:
  | key | name | regex | detection files |
  | :--- | :--- | :--- | :--- |
  | ue4ss | UE4SS | `ue4ss\|unreal engine 4/5 scripting` | ue4ss.dll |
  | red4ext | RED4ext | `red4ext` | red4ext.dll |
  | cet | Cyber Engine Tweaks | `cyber engine tweaks` | cyber_engine_tweaks.asi |
  | redscript | redscript | `\bredscript\b` | scc.exe |
  | skse | SKSE | `\bskse(64\|vr)?\b\|skyrim script extender` | skse64_loader.exe, skse_loader.exe, sksevr_loader.exe |
  | f4se | F4SE | `\bf4se\b\|fallout 4 script extender` | f4se_loader.exe |
  | sfse | SFSE | `\bsfse\b\|starfield script extender` | sfse_loader.exe |
  | nvse | xNVSE | `\bx?nvse\b\|new vegas script extender` | nvse_loader.exe |
  | obse | OBSE | `\bobse(64)?\b\|oblivion script extender` | obse_loader.exe, obse64_loader.exe |
  | bepinex | BepInEx | `bepinex` | bepinex.core.dll, bepinex.dll, bepinex.preloader.dll |
  | melonloader | MelonLoader | `melon ?loader` | melonloader.dll |
  | smapi | SMAPI | `\bsmapi\b\|stardew ?modding ?api` | stardewmoddingapi.dll, stardewmoddingapi.exe |
  | me2 | Mod Engine 2 | `mod ?engine ?2` | modengine2_launcher.exe |
- Grouping (L3036-3081). Group keys are what `ack` stores:
  - `tool:<key>` when the requirement matches a tool.
  - `nexus:<modId>` for a same-game Nexus mod.
  - `nexus:<gameId>:<modId>` for another game's mod.
  - else `url:<normalised url or lower-cased name>`.
  - A requirement on the mod itself is skipped. URLs are normalised: scheme and `www.` stripped, GitHub cut to owner/repo. One URL is kept per site, and the most specific one wins.
- Tool detection (L3084-3120):
  - Scans the game folder (depth 8) for the tool files. Files listed in the current deploy manifest are skipped, so a disabled mod's files still in the game folder do not count.
  - Then scans each enabled mod's staging dir, labelled `provided by "<name>"`.
- Status per group (L3122-3151), in precedence order:
  1. `ok`: a listed Nexus mod is installed and enabled.
  2. `ok`: the tool file was found.
  3. `ok`: **the key is acked** ("marked as installed").
  4. `disabled`: the mod is installed but disabled (hint `enable <id>`).
  5. `missing`: a still-available Nexus id exists (hint `install <id>`).
  6. `missing`: a tool group whose file was not found.
  7. `external`: has URLs and cannot be verified.
  8. `gone`: only points to a Nexus mod that no longer exists.
  Rows are sorted missing < disabled < external < gone < ok, then by name. Fields: `status␟key␟name␟hint␟urls(space)␟by("; ")␟detail`.

### Reports (`deps_report` L3183-3226)
- `full` mode (used by `install`, `info`, `deps`): every row is shown.
- `problems` mode (after deploy and collection install): `ok` and `gone` rows are hidden. If there are no missing, disabled or external rows, it prints only "All N requirement(s)… present" (when logged in).
- After a deploy, `deps_check_enabled` runs once per process (`DEPS_SHOWN`, L2885, L3229-3236). It never fails the caller.
- `deps --json` (L3543-3550): `[{status,key,name,hint,urls[],by[],detail,acked}]` for enabled mods.

### ack / unack (L3506-3531)
- Stored in `GDATA/deps-ack.json`, a JSON array of group keys (deduplicated with `unique`). It is created as `[]` when missing (L3501-3502).
- `ack <name>` matches only rows whose status is `external` or `missing`. `unack <name>` matches only keys already in the ack list.
- Matching: an exact name (case-insensitive) or exact key wins at once. Otherwise a substring match on the name must be unique, or the command fails.
- Effect: an acked key reports `ok / "marked as installed"` in every report (full, problems, json). It ranks below an installed Nexus mod or a found tool file and above disabled or missing (L3132-3133). In problems mode it is therefore hidden. Acks are per game and never expire.

---

## 13. Nexus API, downloads, rate limit, adult content
- Endpoints: REST `https://api.nexusmods.com/v1` and GraphQL `https://api.nexusmods.com/v2/graphql` (L31-33).
- `api_request` (L443-481):
  - Headers: `Accept: application/json`, `Application-Name: modman`, `Application-Version: 3.2.0`, `User-Agent: modman/3.2.0 (Linux; bash)`, `apikey: <key>`. A body is sent with the JSON content type. Timeout 90 s.
  - **Rate limit**: `x-rl-hourly-remaining` and `x-rl-daily-remaining` are saved to the `ratelimit` file on every response that carries them.
  - HTTP 429 is retried after waiting attempt×15 s (15, 30, 45), with a warning, up to 4 attempts.
  - 5xx and network errors are retried after attempt×2 s, up to 4 attempts.
  - Other non-2xx codes fail at once.
  - GraphQL responses with a non-empty `errors` are failures (L494-503).
- `updates` stops early when the hourly remaining is below 20 (L3751-3754). `status` shows `ratelimit_summary` (L505-509, L4337).
- Account: `GET /users/validate.json` sets `NEXUS_USER` and `IS_PREMIUM` (L511-517). `login` reads the key (hidden input on a tty), strips whitespace, validates it, saves it, and reports the tier. If no games are set up, it runs first-time setup when interactive (L4176-4219). `logout` clears the key, user and premium flag (L4221-4225). The key is stored in plaintext in `config.cfg` (chmod 600).
- Download link: `GET /games/<g>/mods/<m>/files/<f>/download_link.json[?key=&expires=]`. The `PREFERRED_CDN` entry is chosen by `short_name` or `name`, else the first entry. The URI is percent-encoded for space, `[`, `]` and `"` (L1468-1479, L117-121).
- `download_nexus_file` (L1482-1512):
  1. Reuses an existing `.meta` download.
  2. Non-premium users without a key go through `nxm_acquire`.
  3. The file name comes from the URI path, url-decoded. If it has no dot, `Content-Disposition` from a HEAD request is used, else `<mod>-<file>`.
  4. An existing file whose md5 does not match is deleted.
  5. `fetch_file` (L1443-1466): resumes `.part` (`curl -C -`), up to 3 tries. The md5 is verified when known. The `.part` is renamed on success.
  6. Writes `.meta/<mod>-<file>.json` = `{file, md5, modId, fileId, game, downloaded}`.
- **Adult content**: config `adult` (VIEW_ADULT) is sent only as `viewAdultContent` in the collection GraphQL calls `fetch_collection` (L2176) and `collection info` (L2681). It is not sent for `collection update` (hard-coded false, L2718), `search` (L3823-3826) or mod REST calls.
- `search` (L3815-3846): GraphQL `mods(filter:{gameDomainName EQUALS, nameStemmed MATCHES}, sort relevance DESC, count:25)`. `--json` adds `installed` per node.

---

## 14. nxm register and handle flow
- **register** (L4230-4250):
  - Needs `xdg-mime`.
  - Writes `$XDG_DATA_HOME/applications/modman-nxm.desktop` with `Exec="<abs path of script>" nxm %u`, `MimeType=x-scheme-handler/nxm;x-scheme-handler/nxm-protocol;`, `NoDisplay=true` and `Terminal=false`.
  - Runs `xdg-mime default modman-nxm.desktop` for both schemes, then `update-desktop-database`.
- **unregister** (L4252-4257): deletes the desktop file and refreshes the database. It does not reset the xdg-mime default.
- **status / registered check** (L1534-1537, L4312-4314): `xdg-mime query default x-scheme-handler/nxm == modman-nxm.desktop`.
- **Link formats** (`parse_nxm` L1517-1531):
  - `nxm://<game>/mods/<mod>/files/<file>?key=…&expires=…`
  - `nxm://<game>/collections/<slug>[/revisions/<n>]`
  - The game is lower-cased.
- **handle** (`nxm_handle` L4276-4306):
  1. Logs the link without the query string. An unknown link sends a notification and fails.
  2. For a mod link, when `nxm-queue/waiting.pid` holds a live PID, the link is written atomically to `nxm-queue/<game>-<mod>-<file>.nxm` and the handler returns.
  3. If stdin and stdout are a tty, it runs `install <url>` or `collection install <url>` inline.
  4. Otherwise it tries `open_terminal` (L4259-4274): `$TERMINAL`, konsole, gnome-terminal, kitty, alacritty, wezterm, foot, xfce4-terminal, tilix, xterm. The new terminal runs `modman install|collection install <url>`, then "Press Enter to close".
  5. If no terminal is available: a mod link triggers a notification, a background install with `ASSUME_YES=1` logged to nmm.log, and a success or failure notification. A collection link only notifies "Open a terminal and run …".
- **Waiting side** (`nxm_acquire` L1540-1558, free accounts):
  1. Fails when non-interactive unless `MODMAN_NXM_HEADLESS=1`.
  2. Warns if not registered.
  3. Writes `waiting.pid`, deletes any stale queue file, and opens `https://www.nexusmods.com/<game>/mods/<mod>?tab=files&file_id=<file>&nmm=1`.
  4. Polls every 1 s for up to `MODMAN_NXM_TIMEOUT` (900 s).
  5. Parses the link to get key and expires, then deletes the queue file.
  `waiting.pid` is removed on exit if it holds this process's PID (L176).
- **AMBIGUITY A7:** Parallel Premium downloads never wait for nxm links. Free downloads run one at a time, so only one waiter exists. A link for a different file goes to a different queue file and is never consumed (the handler still returns success).

---

## 15. Auto-update state machine (L4381-4769)

Constants: unit name `modman-autoupdate`. `AU_SETTLE=300` s: the package database must have been quiet this long. `AU_RETRY=10800` s: after a failure, retry at most every 3 h (L4393-4395).

**System fingerprint** (`au_system_fp` L4422-4434): the md5 (first 16 hex chars) of three parts:
- the `ID VERSION_ID BUILD_ID IMAGE_ID IMAGE_VERSION VARIANT_ID` lines of `/etc/os-release` and `/usr/lib/os-release`;
- the newest mtime of each existing path in the system list (`au_system_paths` L4403-4412: os-release files, pacman/rpm/sysimage-rpm/dpkg status/apk/xbps/portage/nix profile/`/run/current-system`/ostree deploy/steamos-release), plus the nixos symlink target. Lock and shm files are ignored (L4416-4419);
- `uname -r`.
The function also returns the newest mtime overall.

**Package manager busy** (L4436-4441): `/var/lib/pacman/db.lck` exists, or any `/proc/*/comm` is in {pacman dnf dnf5 yum rpm apt apt-get dpkg zypper rpm-ostree apk xbps-install emerge steamos-atomupd rauc}.

**Game running** (L4458-4463): a `/proc/*/cmdline` contains the exact NUL-separated arg `AppId=<appid>`, or contains `<G_FOLDER>/` or its backslash form.

**State file** `GDATA/autoupdate.json` (L4465-4471). All values are strings (`jq --args`).
| key | meaning |
| :--- | :--- |
| `system` | fingerprint at the last success or baseline |
| `build` | Steam buildid at the last success or baseline |
| `ok_at` | epoch of the last success or baseline |
| `tried_at` | epoch of the last real attempt (not set when forced) |
| `failed` | `"1"` after a failed attempt, `"0"` after a success |

**`autoupdate run [--force|-f] [--dry-run|-n]`** (L4474-4519):
1. Takes the non-blocking flock on `autoupdate.lock`. If the lock is busy, it returns 0 silently.
2. Unless forced: if the package manager is busy, or the newest system change is less than 300 s old, it logs "system is being updated" and returns 0.
3. For each configured game, it runs `autoupdate_game` in a subshell. Exit codes: 0 = nothing done, 10 = acted, other = failed. The last stdout line is the summary.
4. If nothing acted and nothing failed, it returns 0 (printing "Nothing to do." when forced). Otherwise it prints the "Auto-update summary", calls `notify` with the summary (unless dry run), and the exit status is 0 only if no game failed.

**`autoupdate_game`** (L4522-4604). It sets FORCE_GAME, `ASSUME_YES=1` and `AUTO_DEPLOY=false`. All progress goes to stderr.
| Step | Condition | Result |
| :--- | :--- | :--- |
| 1 | not deployed (`deployment_active` false) | return 0 |
| 2 | collect reasons: `system updated` (fingerprint ≠ state.system), `game updated (build N)` (build present and ≠ state.build), `scheduled check` (interval>0 and now-ok_at ≥ interval h) | — |
| 3 | no ok_at and no state.system, not forced (**first run / baseline**) | write system, build and ok_at (unless dry); return 0 |
| 4 | forced | add `forced` |
| 5 | no reasons | return 0 |
| 6 | not forced, failed=="1" and now-tried_at < 3 h | return 0 (back-off) |
| 7 | game folder unset or missing | log WARN, return 0 (state unchanged, retried) |
| 8 | Steam StateFlags present and ≠ 4 | log INFO "Steam is still updating", return 0 |
| 9 | game running | log INFO "is running — will retry", return 0 |
| 10 | dry run | print "would update and redeploy"; return 10 (no state change) |
| 11 | not forced | set tried_at=now |
| 12 | API key set: refresh tier if unknown. **Premium**: if `autoupdate-collections`, run `collection update <slug> --optional none` for each installed collection, then `updates --install`. **Free**: `updates` (check only; errors ignored) | errors are collected |
| 13 | always | `deploy` (errors collected) |
| 14 | any errors | set failed=1; summary "auto-update had problems (…)"; return 1 |
| 15 | success | set system, build, ok_at=now and failed=0; summary "N mod(s) updated and redeployed (why)" or "mods redeployed (why)"; return 10 |

**Last-run log**: legacy has **no dedicated last-run log file** (**AMBIGUITY A8**). History is recorded as follows:
- `nmm.log` lines from `_log` and `info`/`warn` with levels INFO, WARN and NOTIFY. The summary is logged through `notify` as `NOTIFY` (L159).
- `autoupdate.json` (`ok_at`, `failed`).
- The systemd journal (stderr of the oneshot).
`autoupdate status` (L4723-4758) shows:
- Scheduler: the systemd timer state and next elapse, or cron, or autostart.
- The collections flag, the interval, and the last system change.
- Per deployed game: last auto-update (`ok_at` or "never"), "(last attempt failed)" when failed=="1", the Steam build, and pending reasons (system updated / game updated).
- The hint: "history: modman log".

**enable / disable** (L4606-4721):
- Scheduler selection: systemd user timer (`OnBootSec=3min`, `OnUnitActiveSec=1h`, `RandomizedDelaySec=2min`, `Persistent=true`; the service is a oneshot that runs `"<self>" --no-color -y autoupdate run` with Nice=10 and idle IO), or cron (`@reboot sleep 180` plus a random minute every hour), or an XDG autostart entry (`sleep 120`).
- `enable` removes any old scheduler first and runs `autoupdate run` once to record the baseline.
- The plan marks the `--scheduler`, cron and autostart rows as Drop; they are recorded here only for reference.
- `disable` removes all three.

---

## 16. Behaviours missing from the help text, and ambiguities

### Behaviours missing from the help text
| Behaviour | Detail | Line |
| :--- | :--- | :--- |
| Global lock | `flock -n` on `<cfg>/modman.lock` (fd 9). A busy lock fails with "Another modman command is running". Taken by: deploy, purge, collection install, collection remove, install (local and Nexus), enable/disable, uninstall, reinstall, order, `updates --install`, profile load, clean. **Not** taken by: verify, conflicts, list, info, deps (including the `ack` write), search, game, config, login/logout, mod `download`, profile save/delete, status. `collection update` takes it through `collection_install`. `launch` releases it after its deploy. | L182-185, L1750, L1907, L2350, L2769, L3344, L3352, L3575, L3623, L3647, L3674, L3791, L3876, L3901, L3939 |
| Auto-update lock | separate non-blocking flock; a concurrent run exits 0 | L4484-4485 |
| Rate-limit handling | 429 back-off of 15/30/45 s; 5xx/network back-off of 2/4/6 s; quota file; `updates` stops when hourly < 20; quota shown in `status` | L466-475, L458-463, L3751-3754, L4337 |
| Adult content | `adult` applies only to collection revision queries (install, info); `collection update` forces false | L2176, L2681, L2718 |
| Notifications | `notify-send -a modman -i applications-games` from nxm handling and the auto-update summary; also logged as NOTIFY | L157-160, L4279-4304, L4517 |
| Game-running check | auto-update only; deploy and purge do not check | L4458-4463, L4556 |
| Steam update-in-progress check | auto-update skips while StateFlags ≠ 4 | L4553-4555 |
| Free-account flow | opens the browser and waits for an nxm click (900 s); headless override via env | L1540-1558 |
| Disk-space warning | collection install warns when 2× download size exceeds free space | L2437-2440 |
| Local archive identification | md5_search on Nexus | L3300-3305 |
| Update policy `exact` | collection-pinned mods are skipped by `updates --install` unless `--all` | L3782, L3796 |
| Profile and folder edit guard | `game set folder\|subdir\|profile` and `game remove` refuse while deployed | L4119, L4138-4140 |
| v2 migration and v2 deployment purge | (Drop in the plan) | L235-243, L1933-1947, L4374-4377 |
| Changed-since-deploy hints | list, verify, status and launch compare mtimes | L1987, L3436, L3937, L4364 |
| Archive hardening | deletes links and special files after extraction | L642-644 |
| Variant prompt | multi-variant archives; the pick is stored and reused | L894-920 |
| Hidden commands | `ui-state`, `ui-detect`, `ui-thumbs` (Drop), bare `nxm://` | L5038-5041 |
| `collection <url>` shortcut | treated as install | L2796-2799 |
| Logging | every command line is logged (args redacted for login); the API key is never logged | L5006 |

### Ambiguities (facts only; no design decisions made here)
- **A1** `game set paks-dir` overwrites `G_EXTRA`, which bethesda presets use for `fmt:AppData folder` (L4149 vs L438). The checklist mentions a "plugins format" `game set` key, but none exists.
- **A2** Config `fomod` (FOMOD_MODE) is never consulted by any install path. The effective default for single-mod installs is `auto` behaviour: ask when interactive, else default (L1390, L3324).
- **A3** `purge --legacy` is accepted but has no effect. The v2 purge runs whenever `deployed.manifest` is non-empty (L1900-1912).
- **A4** The `install --fomod` error message lists only ask, default and none, but `reuse` is also accepted (L3340). `reinstall --fomod` is not validated (L3639).
- **A5** The copy-fallback creates duplicate manifest lines for one target (the last one wins). Files that failed to place still have manifest lines (L1825, L1864).
- **A6** `collection update` sends `viewAdultContent:false` regardless of the `adult` setting (L2718).
- **A7** An nxm link for a different file than the one being waited for is written to a queue file nobody reads (L4284-4286, L1542-1555).
- **A8** There is no dedicated auto-update "last-run log" artifact. State is `autoupdate.json`, plus `nmm.log` lines and the journal (L4465-4471, L4491, L4551-4557, L159).
- **A9** `plugins.managed` is written only when plugins were added. It is truncated on every undeploy, and `deploy` always undeploys first, so it reflects only the last deploy (L2099, L1734, L1796-1799).
- **A10** Purge restores backups in `backup.list` order and creates parent dirs as needed. Dir removal is by `sort -ru` on paths, not by depth count (L1713-1731).
- **A11** `order up` raises priority (higher index). "first/bottom/lowest" is position #1, the lowest priority (L3680-3683).
- **A12** `auto` deploy method never selects symlink or reflink-copy directly. It falls back to `copy` (`cp --reflink=auto`) when a hardlink across the staging and game dirs fails (L1577-1581).
