# Graph Report - modman  (2026-11-23)

## Corpus Check
- Large corpus: 464 files · ~512,870 words. Semantic extraction will be expensive (many Claude tokens). Consider running on a subfolder.

## Summary
- 8052 nodes · 23137 edges · 236 communities (210 shown, 26 thin omitted)
- Extraction: 95% EXTRACTED · 5% INFERRED · 0% AMBIGUOUS · INFERRED: 1129 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Frontend App Tests
- Loader Cards UI
- Jobs & Errors
- App Shell
- Tauri Commands
- Frontend Store
- Loader Archive Cache
- Declarative Loaders
- Loader Errors
- Launch & Apply
- Deploy Confinement
- Mods Model
- SMAPI Support
- Nexus Client
- CLI App
- Browse Tiles
- Collection CLI Tests
- Loader Adopt/Preview
- Path Confinement
- Autoupdate
- UI Controls
- Python Scripts
- Integration Migration
- Glob Matching
- Autoupdate Tests
- Games & Settings
- Steam Detection
- CLI Integrations
- Collections
- Requirements
- Fetch & Downloads
- Deploy Tests
- Download Core
- Mod Commands
- Abort Dialogs
- Restage
- Restore
- Nexus Info Script
- FOMOD
- Community 40
- Community 41
- Community 42
- Community 43
- Community 44
- Community 46
- Community 47
- Community 48
- Community 49
- Community 50
- Community 51
- Community 52
- Community 53
- Community 54
- Community 55
- Community 56
- Community 57
- Community 58
- Community 59
- Community 60
- Community 61
- Community 62
- Community 63
- Community 64
- Community 65
- Community 66
- Community 67
- Community 68
- Community 69
- Community 70
- Community 71
- Community 72
- Community 73
- Community 74
- Community 75
- Community 76
- Community 77
- Community 78
- Community 79
- Community 80
- Community 81
- Community 82
- Community 83
- Community 84
- Community 85
- Community 86
- Community 88
- Community 89
- Community 90
- Community 91
- Community 92
- Community 93
- Community 94
- Community 95
- Community 96
- Community 97
- Community 98
- Community 99
- Community 100
- Community 101
- Community 102
- Community 103
- Community 104
- Community 105
- Community 106
- Community 107
- Community 108
- Community 109
- Community 110
- Community 111
- Community 112
- Community 113
- Community 114
- Community 115
- Community 116
- Community 117
- Community 118
- Community 119
- Community 120
- Community 121
- Community 122
- Community 123
- Community 124
- Community 125
- Community 126
- Community 128
- Community 129
- Community 130
- Community 131
- Community 132
- Community 133
- Community 134
- Community 135
- Community 136
- Community 137
- Community 138
- Community 139
- Community 140
- Community 141
- Community 142
- Community 143
- Community 144
- Community 145
- Community 146
- Community 147
- Community 148
- Community 149
- Community 150
- Community 151
- Community 152
- Community 153
- Community 154
- Community 155
- Community 156
- Community 157
- Community 158
- Community 160
- Community 161
- Community 162
- Community 163
- Community 165
- Community 166
- Community 167
- Community 168
- Community 169
- Community 170
- Community 171
- Community 172
- Community 173
- Community 174
- Community 175
- Community 176
- Community 177
- Community 178
- Community 179
- Community 180
- Community 181
- Community 182
- Community 183
- Community 184
- Community 185
- Community 186
- Community 187
- Community 188
- Community 189
- Community 190
- Community 191
- Community 192
- Community 193
- Community 194
- Community 195
- Community 196
- Community 197
- Community 198
- Community 199
- Community 200
- Community 201
- Community 202
- Community 203
- Community 204
- Community 205
- Community 206
- Community 207
- Community 208
- Community 209
- Community 211
- Community 212
- Community 213
- Community 214
- Community 215
- Community 216
- Community 217
- Community 218
- Community 219
- Community 220
- Community 221
- Community 222
- Community 224
- Community 225
- Community 226
- Community 227
- Community 228
- Community 230
- Community 231
- Community 232
- Community 234
- Community 235

## God Nodes (most connected - your core abstractions)
1. `Store` - 216 edges
2. `LoaderError` - 122 edges
3. `Cx` - 120 edges
4. `Paths` - 96 edges
5. `CmdResult` - 88 edges
6. `DeployError` - 88 edges
7. `useUi` - 87 edges
8. `NexusError` - 85 edges
9. `cx()` - 84 edges
10. `call()` - 84 edges

## Surprising Connections (you probably didn't know these)
- `Legacy deps (requirements) analysis and ack` --semantically_similar_to--> `Loader routes (distribution routes per loader)`  [INFERRED] [semantically similar]
  docs/plan/20261020-rust-migration/legacy_spec.md → .github/prompts/add-game-support.md
- `Installer redirect (Nexus installer-only page -> GitHub release)` --semantically_similar_to--> `Mod loaders (SMAPI) and security model`  [INFERRED] [semantically similar]
  .github/prompts/add-game-support.md → docs/loaders.md
- `analyze()` --references--> `group`  [INFERRED]
  crates/vessel-core/src/requirements.rs → .github/workflows/ci.yml
- `CSP-compatible image/YouTube/style handling` --semantically_similar_to--> `Strict CSP (bundled fonts, no inline style)`  [INFERRED] [semantically similar]
  docs/plan/20261031-browse-games-ui-fixes/T2-findings.md → DESIGN.md
- `Ignored MODMAN_* environment variables` --references--> `Legacy modman v3.2.0 behaviour spec`  [INFERRED]
  README.md → docs/plan/20261020-rust-migration/legacy_spec.md

## Import Cycles
- 1-file cycle: `apps/desktop/src-tauri/src/events.rs -> apps/desktop/src-tauri/src/events.rs`
- 1-file cycle: `crates/vessel-core/src/app.rs -> crates/vessel-core/src/app.rs`
- 1-file cycle: `crates/vessel-core/src/app/restore.rs -> crates/vessel-core/src/app/restore.rs`
- 1-file cycle: `crates/vessel-core/src/archive.rs -> crates/vessel-core/src/archive.rs`
- 2-file cycle: `crates/vessel-core/src/app/restore.rs -> crates/vessel-core/src/mods/install.rs -> crates/vessel-core/src/app/restore.rs`
- 2-file cycle: `crates/vessel-core/src/autoupdate.rs -> crates/vessel-core/src/autoupdate/units.rs -> crates/vessel-core/src/autoupdate.rs`
- 2-file cycle: `crates/vessel-core/src/loaders.rs -> crates/vessel-core/src/loaders/declarative.rs -> crates/vessel-core/src/loaders.rs`
- 2-file cycle: `crates/vessel-core/src/loaders.rs -> crates/vessel-core/src/loaders/registry.rs -> crates/vessel-core/src/loaders.rs`
- 2-file cycle: `crates/vessel-core/src/collections.rs -> crates/vessel-core/src/collections/db.rs -> crates/vessel-core/src/collections.rs`
- 2-file cycle: `crates/vessel-core/src/deploy.rs -> crates/vessel-core/src/deploy/plan.rs -> crates/vessel-core/src/deploy.rs`
- 2-file cycle: `crates/vessel-core/src/deploy.rs -> crates/vessel-core/src/deploy/quarantine.rs -> crates/vessel-core/src/deploy.rs`
- 2-file cycle: `crates/vessel-core/src/fomod.rs -> crates/vessel-core/src/fomod/apply.rs -> crates/vessel-core/src/fomod.rs`
- 2-file cycle: `crates/vessel-core/src/fomod.rs -> crates/vessel-core/src/fomod/plan.rs -> crates/vessel-core/src/fomod.rs`
- 2-file cycle: `apps/desktop/src/app/bridge.ts -> apps/desktop/src/app/run.ts -> apps/desktop/src/app/bridge.ts`
- 2-file cycle: `crates/vessel-cli/src/context.rs -> crates/vessel-cli/src/ui.rs -> crates/vessel-cli/src/context.rs`
- 2-file cycle: `crates/vessel-core/src/config.rs -> crates/vessel-core/src/paths.rs -> crates/vessel-core/src/config.rs`
- 2-file cycle: `crates/vessel-core/src/migrate.rs -> crates/vessel-core/src/migrate/integrations.rs -> crates/vessel-core/src/migrate.rs`
- 2-file cycle: `crates/vessel-core/src/migrate.rs -> crates/vessel-core/src/migrate/marker.rs -> crates/vessel-core/src/migrate.rs`
- 2-file cycle: `crates/vessel-core/src/games/definitions.rs -> crates/vessel-core/src/loaders/declarative.rs -> crates/vessel-core/src/games/definitions.rs`
- 2-file cycle: `crates/vessel-core/src/games.rs -> crates/vessel-core/src/games/definitions.rs -> crates/vessel-core/src/games.rs`

## Hyperedges (group relationships)
- **Game support notes produced by the add-game-support workflow** — _github_prompts_add_game_support, docs_game_support_cyberpunk2077, docs_game_support_schedule1, docs_game_support_stardewvalley [EXTRACTED 0.95]
- **SMAPI loader consent, run, backup and rollback flow** — docs_loaders_pending_preview_consent, docs_loaders_installer_security_model, docs_loaders_backup_rollback, docs_loaders_provenance [EXTRACTED 0.95]
- **Legacy deploy, purge and plugins.txt lifecycle** — docs_plan_20261020_rust_migration_legacy_spec_deploy_purge, docs_plan_20261020_rust_migration_legacy_spec_plugins_txt, docs_plan_20261020_rust_migration_t17_report_g1_plugins_txt_purge [INFERRED 0.85]
- **Browse/Games UX fix workstream (diagnosis, fixes, validation)** — docs_plan_20261031_browse_games_ux_t1_findings, docs_plan_20261031_browse_games_ux_t4_validation, docs_plan_20261031_browse_games_ux_t5_validation, docs_plan_20261031_browse_games_ux_t6_findings [INFERRED 0.85]
- **Nexus game support approach (catalogue, import, generic fallback, promotion)** — docs_plan_20261101_nexus_game_support_plan_nexus_catalogue, docs_plan_20261101_nexus_game_support_plan_vortex_import_tool, docs_plan_20261101_nexus_game_support_plan_generic_fallback, docs_plan_20261101_nexus_game_support_plan_promotion_to_tested [EXTRACTED 1.00]

## Communities (236 total, 26 thin omitted)

### Community 0 - "Frontend App Tests"
Cohesion: 0.03
Nodes (110): noteRecovered(), resetBridge(), startBridge(), unlisteners, makeQueryClient(), PICTURES, openPendingLoaders(), NO_FILTER (+102 more)

### Community 1 - "Loader Cards UI"
Cohesion: 0.02
Nodes (194): Props, LoaderCards(), LoaderCardView(), account(), appInfo(), autoupdateDisable(), autoupdateEnable(), autoupdateRuns() (+186 more)

### Community 2 - "Jobs & Errors"
Cohesion: 0.05
Nodes (155): errMsg(), isCancelError(), jobLabel(), JobOutcome, jobOutcome(), LABEL, assertNever(), useActiveGame() (+147 more)

### Community 3 - "App Shell"
Cohesion: 0.03
Nodes (102): AccountChip(), App(), Brand(), GameContext(), ICON, ToastSlot(), TopBar(), useClosingToast() (+94 more)

### Community 4 - "Tauri Commands"
Cohesion: 0.05
Nodes (80): account(), app_info(), AppInfo, autoupdate_disable(), autoupdate_runs(), autoupdate_status(), backups_clean(), backups_poisoned() (+72 more)

### Community 5 - "Frontend Store"
Cohesion: 0.03
Nodes (73): ActivityEntry, AddGamePrefill, BrowseSession, capJobs(), CollectionFilter, displayLive(), displayToast(), DownloadEntry (+65 more)

### Community 6 - "Loader Archive Cache"
Cohesion: 0.07
Nodes (84): ARCHIVE_CHANGED, at_pre(), baseline(), cache_archive(), cached_archive(), check_op(), check_token(), clear_install_backups() (+76 more)

### Community 7 - "Declarative Loaders"
Cohesion: 0.05
Nodes (44): Aliases, BEPINEX_FILES, bepinex_fixture_matches_and_plans_deploy_and_launch(), BEPINEX_TOML, check_repo(), ctx(), DeclarativeLoader, dests() (+36 more)

### Community 8 - "Loader Errors"
Cohesion: 0.05
Nodes (55): LoaderError, AlreadyInstalled, AmbiguousMatch, ArchiveRequired, CompatToolMissing, Conflict, Consent, DuplicateKey (+47 more)

### Community 9 - "Launch & Apply"
Cohesion: 0.06
Nodes (76): accounts(), ack_and_apply_clear_the_mismatch(), add_dll(), apply(), apply_backs_up_writes_and_records(), apply_with(), apply_without_desired_only_clears_what_vessel_applied(), ApplyOutcome (+68 more)

### Community 10 - "Deploy Confinement"
Cohesion: 0.08
Nodes (63): confine(), confine_refuses_symlinked_folder_pointing_outside(), is_escape(), DeployEnv, DeployError, GameRunning, Games, InjectedCrash (+55 more)

### Community 11 - "Mods Model"
Cohesion: 0.06
Nodes (74): ambiguous_substring_lists_every_match(), changed_key(), changed_since(), collection_mod_ids(), collections_of(), enable_disable_and_change_tracking(), exact_id_beats_nexus_id_and_substring(), exact_name_is_case_insensitive_and_beats_substring() (+66 more)

### Community 12 - "SMAPI Support"
Cohesion: 0.06
Nodes (62): adopt_uninstalls_verifies_then_installs(), bad_layouts_are_hard_rejects(), BUNDLE_DLL, bundled_mod_dirs(), BUNDLED_MOD_IDS, check_bundle(), classify_vanilla_outcomes(), contains() (+54 more)

### Community 13 - "Nexus Client"
Cohesion: 0.05
Nodes (37): Account, CdnLink, DEFAULT_GRAPHQL_URL, DEFAULT_REST_BASE, domain_validation_blocks_path_injection(), encode_uri(), error_message(), error_message_prefers_json_message() (+29 more)

### Community 14 - "CLI App"
Cohesion: 0.05
Nodes (29): Mods, chain(), Cx<'a>, error_chain_joins_sources_once(), game_errors_carry_the_cli_hints(), games_hint(), Headless, human_size() (+21 more)

### Community 15 - "Browse Tiles"
Cohesion: 0.06
Nodes (63): browseDefaults(), collectionInstalled(), compact(), FOCUS_ATTR, InstalledBadge(), MOD_GRID, MOD_PICTURE, modInstalled() (+55 more)

### Community 16 - "Collection CLI Tests"
Cohesion: 0.06
Nodes (48): collection_info_and_bad_references(), collection_install_applies_choices_rules_and_skips_optional(), collection_remove_uninstalls_exclusive_mods(), collection_update_removes_dropped_mods_then_remove_keep_mods_keeps_them(), download_only_fetches_archives_without_installing(), Fake, FOMOD_XML, G (+40 more)

### Community 17 - "Loader Adopt/Preview"
Cohesion: 0.10
Nodes (62): cached_origin(), preview(), PreviewRequest, adopt_is_blocked_when_the_game_is_not_vanilla_after_uninstall(), adopt_takes_over_a_manual_install(), bad_tokens_never_spawn(), base(), cache_archive_refuses_bytes_other_than_the_expected_sha() (+54 more)

### Community 18 - "Path Confinement"
Cohesion: 0.07
Nodes (51): bad_component(), components(), components_reject_traversal_absolute_and_nul(), Confined, DIR_FLAGS, LoadOrderError, Deploy, Io (+43 more)

### Community 19 - "Autoupdate"
Cohesion: 0.06
Nodes (48): Account, Free, None, Premium, Actions, Busy, AlreadyRunning, SystemUpdating (+40 more)

### Community 20 - "UI Controls"
Cohesion: 0.07
Nodes (43): Desktop app HTML entry (index.html), PageStep, BADGE, BadgeTone, DOT, DotTone, Line, Lines() (+35 more)

### Community 21 - "Python Scripts"
Cohesion: 0.06
Nodes (12): Handler, Listing, run_main(), Sources, zip_bytes(), FakeOpener, FakeResp, headers() (+4 more)

### Community 22 - "Integration Migration"
Cohesion: 0.05
Nodes (47): env(), first_start_replaces_legacy_timer_and_nxm_handler(), write(), which(), LEGACY_APP_DIR, LEGACY_APP_ID, LEGACY_DB_FILE, LEGACY_DBUS_NAME (+39 more)

### Community 23 - "Glob Matching"
Cohesion: 0.04
Nodes (49): Glob, glob_semantics(), match_component(), match_segments(), split(), NewLoaderRun, DeployFiles, DownloadOrigin (+41 more)

### Community 24 - "Autoupdate Tests"
Cohesion: 0.07
Nodes (35): acf(), back_off_three_hours_after_failure(), collections_skipped_when_setting_disabled(), concurrent_run_returns_at_once(), dry_run_changes_no_state(), failed_redeploy_marks_failed_and_exits_1(), FakeActions, FakeHost (+27 more)

### Community 25 - "Games & Settings"
Cohesion: 0.08
Nodes (48): remove(), check_rel_dir(), ACTIVE_GAME_KEY, apply_setting(), builtin_profiles_are_known(), check_game_dirs(), deployment_active(), domain_validation() (+40 more)

### Community 26 - "Steam Detection"
Cohesion: 0.07
Nodes (52): apply_steam_app(), app_in_library(), AppManifest, BUSY_OR_BROKEN, canonical_sorted(), compat_tool_mapping_reads_per_app_and_global_default(), compat_tools_and_runtime_resolve_from_libraries(), CompatTool (+44 more)

### Community 27 - "CLI Integrations"
Cohesion: 0.07
Nodes (40): cli(), CLI, Ctl, defaults(), disabled_legacy_timer_is_installed_but_not_enabled(), enable_call(), enabled_legacy_timer_is_replaced_with_same_interval(), foreign_nxm_default_is_left_alone() (+32 more)

### Community 28 - "Collections"
Cohesion: 0.07
Nodes (41): CollectionError, Archive, BadRef, Download, InvalidManifest, Io, Mods, Nexus (+33 more)

### Community 29 - "Requirements"
Cohesion: 0.08
Nodes (49): add(), ack(), acks(), add_url(), analyze(), collection_extras(), counts(), declared_loader() (+41 more)

### Community 30 - "Fetch & Downloads"
Cohesion: 0.06
Nodes (35): a_redirect_to_a_refused_host_is_not_followed(), an_announced_oversize_body_is_refused_before_download(), an_oversize_stream_is_aborted_and_its_part_deleted(), Attempt, Fatal, Retry, client(), content_disposition_names() (+27 more)

### Community 31 - "Deploy Tests"
Cohesion: 0.13
Nodes (42): assert_original_exe(), backups_are_content_addressed_and_released_after_purge(), crash_rollback_restores_original_when_mod_file_is_byte_identical(), cross_device_staging_skips_hardlink_and_reflink(), deploy_and_purge_recover_automatically_after_a_crash(), deploy_and_purge_refused_while_game_running(), deploy_and_purge_refused_while_lock_held(), deploy_crash_after_k_files_rolls_back_on_next_open() (+34 more)

### Community 32 - "Download Core"
Cohesion: 0.06
Nodes (40): DownloadError, BadUrl, ChecksumMismatch, Http, InvalidNxm, Io, Network, NotFound (+32 more)

### Community 33 - "Mod Commands"
Cohesion: 0.09
Nodes (47): dispatch(), action_json(), changed_hint(), clean(), CleanArgs, conflicts(), ConflictsArgs, deps() (+39 more)

### Community 34 - "Abort Dialogs"
Cohesion: 0.10
Nodes (37): fallbacks(), fallbacks_prefer_dialogs_and_pass_the_message_verbatim(), gtk_dialog(), show(), TITLE, colour_is_forced_only_without_no_color_settings(), CONFIG_KEYS, config_lists_the_nine_keys_with_defaults() (+29 more)

### Community 35 - "Restage"
Cohesion: 0.11
Nodes (31): all(), ambiguous_match_goes_to_backup(), ambiguous_poisoned_backups_warn_but_do_not_block(), best_match(), CarriedFile, carry_over(), confirmed_poisoned_backup_blocks_and_changes_nothing(), failure_before_swap_keeps_old_tree_and_rows() (+23 more)

### Community 36 - "Restore"
Cohesion: 0.12
Nodes (45): a_failed_write_back_leaves_nothing_and_others_still_restore(), archive(), bulk_restore_puts_mods_back_at_their_positions(), capture(), deploy(), deployed(), failed(), install() (+37 more)

### Community 38 - "Nexus Info Script"
Cohesion: 0.09
Nodes (37): build_opener(), check_url(), cmd_contents(), cmd_files(), cmd_key_status(), cmd_mod(), cmd_requirements(), cmd_top() (+29 more)

### Community 39 - "FOMOD"
Cohesion: 0.07
Nodes (28): arr(), FomodChoices, GroupChoice, OptionChoice, add_files(), ask(), default_selection(), FomodResolver (+20 more)

### Community 40 - "Community 40"
Cohesion: 0.06
Nodes (19): ApiKey, ApiKeyAuth, AuthProvider, key_is_trimmed_and_debug_redacted(), NexusConfig, RetryPolicy, with_base_builds_both_endpoints(), client() (+11 more)

### Community 41 - "Community 41"
Cohesion: 0.13
Nodes (48): a_fresh_shortcut_uses_the_vessel_appid(), account(), adding_the_shortcut_reuses_the_legacy_entry(), adds_once_keeps_others_and_removes(), APP_NAME, app_name_is(), bad(), crc32() (+40 more)

### Community 42 - "Community 42"
Cohesion: 0.08
Nodes (40): Cx, AboutTarget, Collection, Mod, browse(), browse_installed_set_includes_loader_installs(), BROWSE_PAGE, BrowseKind (+32 more)

### Community 43 - "Community 43"
Cohesion: 0.09
Nodes (38): AutoupdateError, Collections, Flatpak, Games, Io, Mods, Sql, Store (+30 more)

### Community 44 - "Community 44"
Cohesion: 0.10
Nodes (44): cleanup(), copy_dir(), copy_state(), checkpoint(), copy_file_verified(), copy_tree(), Entry, Dir (+36 more)

### Community 46 - "Community 46"
Cohesion: 0.07
Nodes (10): Ctx, read_line(), Printer, CliUi, fomod_answers_parse_numbers_and_ranges(), open_url(), parse_selection(), render() (+2 more)

### Community 47 - "Community 47"
Cohesion: 0.11
Nodes (37): cached_requirements(), api_key_is_not_forwarded_on_cross_origin_redirect(), auth(), browse_sends_each_index_its_filters(), client(), config(), exhausted_rate_limit_throttles_and_persists_across_clients(), existing_open_config_dir_is_tightened_to_0700() (+29 more)

### Community 48 - "Community 48"
Cohesion: 0.08
Nodes (39): BUILTIN, builtin_definitions(), cyberpunk_declares_red4ext_cet_and_redscript_loaders(), definition_with_escaping_subdir_or_paks_dir_is_rejected(), every_other_preset_is_experimental(), find_definition(), GameDefinition, invalid_override_names_the_file() (+31 more)

### Community 49 - "Community 49"
Cohesion: 0.12
Nodes (29): KeyringHook, abort_keeps_a_lock_file_it_did_not_create(), assert_migrated(), Bus, crash_after_any_step_converges_on_rerun(), custom_data_dir_setting_is_preserved(), existing_vessel_dir_without_marker_is_a_conflict(), failed_db_rewrite_leaves_vessel_db_unchanged() (+21 more)

### Community 50 - "Community 50"
Cohesion: 0.10
Nodes (30): ConfirmAnswer, ConfirmSpec, AfterChangeView, a_checked_option_deletes_and_reports_the_archives(), Asker, FomodStepView, headless_uninstall_proceeds_and_keeps_archives(), left() (+22 more)

### Community 51 - "Community 51"
Cohesion: 0.05
Nodes (39): Cli, Commands, Autoupdate, Clean, Collection, Config, Conflicts, Deploy (+31 more)

### Community 52 - "Community 52"
Cohesion: 0.08
Nodes (39): Condition, condition(), Dependency, Composite, File, Flag, Other, FileEntry (+31 more)

### Community 53 - "Community 53"
Cohesion: 0.16
Nodes (37): a_plain_403_is_not_a_rate_limit(), an_allowlisted_redirect_is_followed(), API_BASE, api_body_over_one_mib_is_rejected(), API_MAX_BYTES, API_PATH, assert_empty(), asset() (+29 more)

### Community 54 - "Community 54"
Cohesion: 0.04
Nodes (44): assumptions, baseline, acceptance_criteria, captured_at, objective, created_at, created_by, decisions (+36 more)

### Community 55 - "Community 55"
Cohesion: 0.09
Nodes (26): PluginRule, io_err(), cache_dir(), cached_masterlist(), DEFAULT_LOOT_BASE_URL, download(), fetch_masterlist(), FETCH_TIMEOUT (+18 more)

### Community 56 - "Community 56"
Cohesion: 0.10
Nodes (18): nxm_submit(), focus_main(), run(), a_forwarded_valid_url_is_parsed_queued_and_redacted(), argv(), argv_must_be_the_forwarded_shape(), forwarded_malformed_or_foreign_urls_are_rejected_without_enqueue(), on_second_instance() (+10 more)

### Community 57 - "Community 57"
Cohesion: 0.08
Nodes (24): an_answer_unblocks_the_asking_thread(), Answer, Cancel, Choice, Choices, Confirm, Path, Text (+16 more)

### Community 58 - "Community 58"
Cohesion: 0.11
Nodes (35): LoaderFile, backup(), BackupRef, backups_are_copies_not_hardlinks(), backups_are_recorded_for_the_run(), by_path(), changed(), check_deployed() (+27 more)

### Community 59 - "Community 59"
Cohesion: 0.05
Nodes (43): assumptions, baseline, acceptance_criteria, captured_at, objective, created_at, created_by, decisions (+35 more)

### Community 60 - "Community 60"
Cohesion: 0.15
Nodes (20): restage_keeps_edits_and_blocks_on_poisoned_backups(), apply_refuses_paths_escaping_the_game_folder(), apply_takes_only_confirmed_entries_and_purge_then_removes_the_file(), apply_with_ambiguous_moves_untracked_files_and_restore_roundtrips(), apply_without_confirmation_changes_nothing(), BackupRows, backups_in_mod_only_folders_are_confirmed_without_a_record(), dry_run_classifies_poisoned_backups_without_side_effects() (+12 more)

### Community 61 - "Community 61"
Cohesion: 0.07
Nodes (31): BOOLS, Config, ConfigKey, Adult, .ALL, AutoDeploy, AutoupdateCollections, AutoupdateInterval (+23 more)

### Community 62 - "Community 62"
Cohesion: 0.16
Nodes (27): APPDATA, collection_after_rule_is_respected(), collection_rule_applies_when_loot_cannot_read_plugins(), conflict_data_reports_file_and_plugin_conflicts(), deploy_writes_star_plugins_txt_crlf_masters_first_and_backs_up_original(), esm(), esp(), failed_masterlist_fetch_keeps_cache() (+19 more)

### Community 63 - "Community 63"
Cohesion: 0.09
Nodes (17): API_KEY_ENV, Backend, File, Keyring, CREDENTIALS_FILE, CredentialsFile, is_unavailable(), KEYRING_SERVICE (+9 more)

### Community 64 - "Community 64"
Cohesion: 0.07
Nodes (31): Activity, ACTIVITY, DEPLOY_PROGRESS, DeployProgress, Download, DOWNLOAD, emit(), emit_loader_pending() (+23 more)

### Community 65 - "Community 65"
Cohesion: 0.12
Nodes (36): a_redirect_off_the_allowed_hosts_is_refused(), allowed_url(), an_oversize_picture_is_rejected(), base64(), cache_path(), cache_paths_reject_traversal_and_honour_the_ttl(), cached(), collection_art() (+28 more)

### Community 66 - "Community 66"
Cohesion: 0.10
Nodes (31): archive_entry_filetype(), archive_entry_hardlink(), archive_entry_is_encrypted(), archive_entry_mode(), archive_entry_pathname(), archive_entry_pathname_utf8(), ARCHIVE_EOF, archive_error_string() (+23 more)

### Community 67 - "Community 67"
Cohesion: 0.11
Nodes (32): acquire(), attach_cards(), CollectionInfo, CollectionResult, Failure, failures(), info(), install() (+24 more)

### Community 68 - "Community 68"
Cohesion: 0.12
Nodes (30): account_info(), AccountInfo, AUTH_BACKEND_KEY, BASE_URL_ENV, config(), connect(), download_file(), empty_keys_are_rejected_before_any_network_call() (+22 more)

### Community 69 - "Community 69"
Cohesion: 0.11
Nodes (34): allowed_url(), art_dir(), art_url(), ArtKind, Hero, Portrait, cached(), falls_back_to_the_flat_name_and_later_roots() (+26 more)

### Community 70 - "Community 70"
Cohesion: 0.13
Nodes (34): choose_file(), digits(), download_target(), file(), file_rows(), file_rows_default_to_the_primary_file(), fomod_config_key_sets_install_mode_unless_flag_given(), identify() (+26 more)

### Community 71 - "Community 71"
Cohesion: 0.12
Nodes (39): bannerlord_submodule_marker(), bepinex_places_into_bepinex_tree(), bethesda_exe_or_data_with_siblings_targets_game_root(), bethesda_loose_plugin_stays_at_deploy_root(), bethesda_strips_wrapper_and_data_and_drops_docs(), case_insensitive_merge_into_one_canonical_directory(), CET_MODS, cyberpunk_folders_by_content() (+31 more)

### Community 72 - "Community 72"
Cohesion: 0.08
Nodes (15): EchoOff, login(), LoginArgs, logout(), LogoutArgs, read_key(), ConfigArgs, run() (+7 more)

### Community 73 - "Community 73"
Cohesion: 0.12
Nodes (37): acquire(), archives(), entries(), is_hex_id(), is_token_char(), KEEP_RUNS, looks_like_token(), MAX_LOG_AGE (+29 more)

### Community 74 - "Community 74"
Cohesion: 0.09
Nodes (28): p(), result, out, scan(), afterClose(), curView(), expect(), k() (+20 more)

### Community 75 - "Community 75"
Cohesion: 0.09
Nodes (34): afterChangeOf(), applyAfterChange(), applyCollection(), collectionResultOf(), deployedMsg(), gameOf(), loaderPendingOf(), Run (+26 more)

### Community 76 - "Community 76"
Cohesion: 0.05
Nodes (39): app, security, windows, withGlobalTauri, bundleMediaFramework, files, build, beforeBuildCommand (+31 more)

### Community 77 - "Community 77"
Cohesion: 0.11
Nodes (33): an_inherited_appimage_variable_is_ignored(), an_inherited_appimage_variable_is_ignored_for_the_gui(), appimage_of(), canonical(), cli_binary(), cli_binary_prefers_the_sibling_of_the_gui(), Ctl, enable() (+25 more)

### Community 78 - "Community 78"
Cohesion: 0.11
Nodes (31): add_game(), add_rejects_a_subdir_outside_the_game_folder(), add_rejects_a_traversal_typed_at_the_subdir_question(), AddGame, detect(), Detected, folder_path(), GameFolder (+23 more)

### Community 79 - "Community 79"
Cohesion: 0.11
Nodes (20): Blob, BlobStore, fsync_dir(), S_IFLNK, S_IFMT, store_dedups_and_restore_roundtrips_mode_mtime_and_symlinks(), copy_attrs(), hash_copy() (+12 more)

### Community 80 - "Community 80"
Cohesion: 0.12
Nodes (24): check_new_name_absent(), checklist(), dbus_path(), die(), FilePlan, git(), is_excluded(), is_legacy_file() (+16 more)

### Community 81 - "Community 81"
Cohesion: 0.07
Nodes (39): T1 findings: Browse-5 / Browse-6 diagnosis, Browse-5: focus recovery scrolls view on click-away, installed_loaders helper (proposed), Browse-6: loader-installed mod not flagged installed, T4 scroll-fix validation log, T5 plugin-installed validation log, T6 findings: two frontend test failures after T5, Plan: support every Nexus game (+31 more)

### Community 82 - "Community 82"
Cohesion: 0.05
Nodes (39): assumptions, created_at, created_by, decisions, findings, binary, db_path, error_text (+31 more)

### Community 83 - "Community 83"
Cohesion: 0.05
Nodes (39): concurrency, cancel-in-progress, group, name, runs-on, steps, timeout-minutes, env (+31 more)

### Community 84 - "Community 84"
Cohesion: 0.09
Nodes (11): danger_confirm_is_bridged_with_its_fields_and_option(), danger_spec(), fomod_groups_are_bridged_to_prompts_from_a_blocking_thread(), GuiFomod, GuiUi, GuiUi<'a>, non_interactive_danger_confirm_takes_default_yes(), non_interactive_questions_take_defaults_without_prompting() (+3 more)

### Community 85 - "Community 85"
Cohesion: 0.12
Nodes (33): BackupRecord, check_id(), copy_durable(), fsync_dir(), game_copy(), held_backup(), list_quarantines(), MANIFEST (+25 more)

### Community 86 - "Community 86"
Cohesion: 0.10
Nodes (35): ArchiveError, Corrupt, DestinationNotEmpty, Encrypted, ForbiddenEntry, Io, MissingBackend, TooLarge (+27 more)

### Community 88 - "Community 88"
Cohesion: 0.16
Nodes (31): FomodRun, default_name(), detects_fomod_installer(), empty_archive_is_rejected_without_a_row(), find_fomod(), fomod_ask_then_reuse_replays_stored_choices_on_reinstall(), fomod_ask_without_resolver_and_default_mode_use_defaults(), fomod_collection_mode_uses_given_choices_and_never_asks() (+23 more)

### Community 89 - "Community 89"
Cohesion: 0.11
Nodes (27): check_domain(), browse_collection(), BROWSE_COLLECTIONS_QUERY, BROWSE_MODS_QUERY, browse_vars(), BrowseCollection, BrowseCollections, BrowseQuery (+19 more)

### Community 90 - "Community 90"
Cohesion: 0.18
Nodes (19): entries(), ext_lower(), find_files(), has_ext(), has_file_ci(), io_err(), is_doc(), is_doc_is_case_insensitive() (+11 more)

### Community 91 - "Community 91"
Cohesion: 0.11
Nodes (17): keep(), RestoreStore, YesMan, apply(), capture(), CHILD_TABLES, columns(), conflict() (+9 more)

### Community 92 - "Community 92"
Cohesion: 0.15
Nodes (34): config_info_names_the_proton_dir(), config_info_proton_dir(), is_flatpak_steam(), check_owner_chain(), DEFAULT_ONLY, detect(), elf_without_mapping_is_native_even_with_exe(), exe_only_is_proton_via_global_default() (+26 more)

### Community 93 - "Community 93"
Cohesion: 0.07
Nodes (36): dependencies, @fontsource-variable/geist@^5.3.0, @fontsource-variable/geist-mono@^5.3.0, framer-motion@^14.4.0, @phosphor-icons/react@^2.1.10, react@^19.3.1, react-dom@^19.3.1, devDependencies (+28 more)

### Community 94 - "Community 94"
Cohesion: 0.11
Nodes (32): AddGameDialog(), AddGameHost(), domainProblem(), folderProblem(), captureOpeners(), Dialog(), syncInert(), Host() (+24 more)

### Community 95 - "Community 95"
Cohesion: 0.13
Nodes (26): data(), denv(), deploy(), deploy_skips_and_purge_keeps_loader_owned_paths(), files_named(), hand_made_install_gives_an_adopt_preview(), junk_candidate(), junk_roundtrip() (+18 more)

### Community 96 - "Community 96"
Cohesion: 0.11
Nodes (33): desktop_exec_quote(), FORWARD_TIMEOUT, GUI_ARGV0, NXM_DESKTOP_FILE, nxm_exec_line(), NXM_SCHEMES, plain_path_is_quoted_and_u_is_last(), SINGLE_INSTANCE_BUS (+25 more)

### Community 97 - "Community 97"
Cohesion: 0.06
Nodes (35): assumptions, audit_evidence, accessibility, performance, security, created_at, created_by, decisions (+27 more)

### Community 98 - "Community 98"
Cohesion: 0.11
Nodes (25): direct_file_name(), keys(), Ordered, reorder_block(), rule_order(), parse_selection(), direct_file_name_uses_url_segment_or_name(), dropped_members_skips_kept_and_still_listed_mods() (+17 more)

### Community 99 - "Community 99"
Cohesion: 0.08
Nodes (15): DeployReport, io_err(), library_is_derived_only_from_steamapps_common(), library_of(), PurgeReport, RecoveryOutcome, Resumed, RolledBack (+7 more)

### Community 100 - "Community 100"
Cohesion: 0.11
Nodes (26): Parse, parse_module_config(), build_plan(), Eval, find_named(), ask_mode_cancel_aborts(), ask_mode_uses_resolver_and_reasks_invalid_answers(), cfg() (+18 more)

### Community 101 - "Community 101"
Cohesion: 0.17
Nodes (31): auto_deploy_redeploys_after_changes(), clean_deletes_only_unreferenced_downloads_after_confirmation(), conflicts_lists_pairs_filter_and_files(), deps_finds_tool_files_in_game_folder(), deps_reports_external_tools_and_ack_unack_persist(), enable_disable_by_ref_all_and_collection(), FOMOD_XML, fomod_zip() (+23 more)

### Community 102 - "Community 102"
Cohesion: 0.17
Nodes (32): copies_a_directory_source(), detect_kind_uses_extension_then_magic(), directory_source_with_symlink_is_rejected(), encrypted_zip_is_reported(), extract(), extraction_stops_at_file_count_and_size_caps(), extracts_7z_fixture(), extracts_rar_fixture() (+24 more)

### Community 103 - "Community 103"
Cohesion: 0.09
Nodes (24): Asset, fetch_latest_asset(), FetchedAsset, GithubError, Api, Download, InvalidGlob, InvalidRepo (+16 more)

### Community 104 - "Community 104"
Cohesion: 0.13
Nodes (28): apply(), base_prefix(), definition(), entry(), KEYBINDS, lines(), merge(), merge_inserts_before_keybinds_keeping_bytes() (+20 more)

### Community 105 - "Community 105"
Cohesion: 0.11
Nodes (20): applied_version(), deploy_lock_is_exclusive_until_dropped(), file_db_uses_wal_and_foreign_keys(), game_identity_migration_keeps_games_and_their_rows(), latest_version(), migrate(), Migration, MIGRATIONS (+12 more)

### Community 106 - "Community 106"
Cohesion: 0.06
Nodes (32): assumptions, baseline, acceptance_criteria, captured_at, objective, created_at, created_by, decisions (+24 more)

### Community 107 - "Community 107"
Cohesion: 0.09
Nodes (30): devDependencies, jsdom@30.1.2, tailwindcss@4.3.3, @tailwindcss/vite@4.3.3, @tauri-apps/cli@2.12.3, @testing-library/dom@10.4.2, @testing-library/jest-dom@7.0.1, @testing-library/react@16.3.3 (+22 more)

### Community 108 - "Community 108"
Cohesion: 0.10
Nodes (23): ApiError, Collection, DepStatus, DetectedGame, followJob(), Game, headers, JobStatus (+15 more)

### Community 109 - "Community 109"
Cohesion: 0.12
Nodes (21): FileInfo, FileUpdate, ModFiles, BULK_RESERVE, header_str(), header_u32(), headers(), parses_headers_and_ignores_responses_without_them() (+13 more)

### Community 110 - "Community 110"
Cohesion: 0.19
Nodes (15): crash_after_mods_txt_write_is_recovered_to_original_bytes(), deploy_merges_before_keybinds_and_purge_restores_original_bytes(), disable_removes_entry_and_user_edits_survive_redeploy(), Fx, GAME, LEGACY, missing_mods_txt_is_created_and_purge_removes_it(), MODS (+7 more)

### Community 111 - "Community 111"
Cohesion: 0.07
Nodes (29): assumptions, created_at, created_by, decisions, global_constraints, constraints, high_risk_signals, relevant_context (+21 more)

### Community 112 - "Community 112"
Cohesion: 0.07
Nodes (28): assumptions, created_at, created_by, decisions, constraints, critic_signals, high_risk_signals, plan_id (+20 more)

### Community 113 - "Community 113"
Cohesion: 0.07
Nodes (28): assumptions, created_at, created_by, decisions, constraints, high_risk_signals, relevant_context, plan_id (+20 more)

### Community 114 - "Community 114"
Cohesion: 0.12
Nodes (22): refreshAll(), LoaderConsent, Block(), Heading(), LoaderConsentDialog(), OP_VERB, originText(), Phase (+14 more)

### Community 115 - "Community 115"
Cohesion: 0.07
Nodes (27): assumptions, created_at, created_by, decisions, constraints, high_risk_signals, plan_id, planner_revision_used (+19 more)

### Community 116 - "Community 116"
Cohesion: 0.11
Nodes (10): Host, ignored(), LiveHost, mtime_secs(), OS_RELEASE_KEYS, PACKAGE_MANAGERS, path_mtime(), steam_state_in() (+2 more)

### Community 117 - "Community 117"
Cohesion: 0.08
Nodes (21): ConfigError, InvalidValue, NoHome, Store, UnknownKey, Error, Archive, Config (+13 more)

### Community 118 - "Community 118"
Cohesion: 0.07
Nodes (26): assumptions, created_at, created_by, decisions, constraints, relevant_context, plan_id, planner_revision_used (+18 more)

### Community 119 - "Community 119"
Cohesion: 0.11
Nodes (6): free_port(), Job, Jobs, main(), run_query(), _keyring_lookup()

### Community 120 - "Community 120"
Cohesion: 0.20
Nodes (16): merge_keeps_first_backup_and_drops_round_trips(), trunc(), check(), deps_rows(), install(), problem_reports_hide_present_requirements(), refresh_deps(), report_msgs() (+8 more)

### Community 121 - "Community 121"
Cohesion: 0.14
Nodes (17): base_prefix(), base_prefix_matches_legacy(), build_plan(), CaseResolver, Conflict, conflicts(), join(), Kind (+9 more)

### Community 122 - "Community 122"
Cohesion: 0.08
Nodes (25): assumptions, created_at, created_by, decisions, constraints, plan_id, planner_revision_used, replan_count (+17 more)

### Community 123 - "Community 123"
Cohesion: 0.15
Nodes (18): account_section(), clean_backups(), CleanBackupsArgs, deploy(), DeployAction, CleanBackups, RestoreQuarantine, DeployArgs (+10 more)

### Community 124 - "Community 124"
Cohesion: 0.14
Nodes (20): app::InstallOpts, CollectionArgs, info(), install(), InstallOpts, list(), PassThrough, passthrough() (+12 more)

### Community 125 - "Community 125"
Cohesion: 0.10
Nodes (13): InvalidMethod, DeployStatus, Faults, Method, .CHAIN, Copy, Hardlink, Reflink (+5 more)

### Community 126 - "Community 126"
Cohesion: 0.13
Nodes (16): distinct_ue4ss_mods_do_not_share_paths(), Fixture, FIXTURES, Fx, install_uses_probed_ue4ss_layout(), installed_ue4ss_mods_have_no_conflicts(), layout(), LEGACY_MODS (+8 more)

### Community 128 - "Community 128"
Cohesion: 0.17
Nodes (21): ConsoleLine(), ConsoleView(), JobIsland(), StateIcon(), Brand(), NAV, Sidebar(), BtnProps (+13 more)

### Community 129 - "Community 129"
Cohesion: 0.16
Nodes (17): AutoupdateArgs, ctl(), disable(), enable(), notify(), options(), run(), run_now() (+9 more)

### Community 130 - "Community 130"
Cohesion: 0.11
Nodes (16): a_rejected_api_key_is_never_echoed_to_the_webview(), CAPS, CARGO, COMMANDS_RS, conf(), CONF, csp_has_no_unsafe_eval_and_no_remote_origins(), directive() (+8 more)

### Community 131 - "Community 131"
Cohesion: 0.16
Nodes (18): Printer, PromptResolver, apply_plan(), ApplyReport, io(), lookup(), parts(), place_file() (+10 more)

### Community 132 - "Community 132"
Cohesion: 0.24
Nodes (10): clean_backups_dry_run_prompt_apply_and_restore_quarantine(), deploy_and_purge_refused_while_game_process_runs(), deploy_dry_run_changes_nothing(), deploy_method_flag_is_recorded_and_validated(), deploy_refused_while_lock_held(), deploy_verify_status_purge_roundtrip_restores_game(), Env, GAME (+2 more)

### Community 133 - "Community 133"
Cohesion: 0.22
Nodes (18): CollectionRecord, CollectionSummary, COLS, detach(), Dropped, dropped_members(), exclusive_members(), get() (+10 more)

### Community 134 - "Community 134"
Cohesion: 0.12
Nodes (19): DESIGN.md: Vessel desktop UI design system, Accent emerald ramp (hue 165), Density: control heights for gamepad/touch, Drift appendix (off-scale values), Live toasts (showProgress/endProgress), Motion tokens and reduced-motion policy, Segmented filter control, Tabs component (roving tabindex, controller mode) (+11 more)

### Community 135 - "Community 135"
Cohesion: 0.11
Nodes (7): lowercase(), download(), DownloadArgs, install(), InstallArgs, reinstall(), ReinstallArgs

### Community 136 - "Community 136"
Cohesion: 0.13
Nodes (18): BASE_PATH, build_command(), check_cmd_safe(), CMD_METACHARS, DEFAULT_TIMEOUT, ENV_ALLOWLIST, LOG_CAP, proton_prefix() (+10 more)

### Community 137 - "Community 137"
Cohesion: 0.18
Nodes (14): checksum_mismatch_retries_then_fails_without_leaving_files(), client_error_is_not_retried(), existing_part_file_resumed_from_its_length(), fetcher(), flaky_server(), head_file_name_from_content_disposition(), interrupted_download_resumes_with_range_and_sha_matches(), mark_done_records_hashes_and_drops_nxm_key() (+6 more)

### Community 138 - "Community 138"
Cohesion: 0.09
Nodes (22): app-id, build-options, append-path, env, command, CARGO_HOME, CARGO_NET_OFFLINE, npm_config_cache (+14 more)

### Community 139 - "Community 139"
Cohesion: 0.34
Nodes (3): definition(), LoaderRoutes, note()

### Community 140 - "Community 140"
Cohesion: 0.13
Nodes (6): app_ready(), CountingHost, loader_recovery(), loader_recovery_runs_once_and_is_empty_on_a_fresh_profile(), startup_check(), startup_runs_the_autoupdate_check_once()

### Community 141 - "Community 141"
Cohesion: 0.26
Nodes (13): after_change(), AfterChange, deploy_after_collection(), deploy_msgs(), empty_deploy_says_nothing_to_deploy(), env_for(), methods_summary(), plugin_stage_msgs() (+5 more)

### Community 142 - "Community 142"
Cohesion: 0.19
Nodes (16): a_valid_link_is_queued_once(), accept(), Accepted, echo(), ECHO_CHARS, is_queued(), link_of(), malformed_and_foreign_links_are_rejected_without_a_row() (+8 more)

### Community 143 - "Community 143"
Cohesion: 0.33
Nodes (20): Onboarding(), Chip(), ConfirmDialog(), Field(), Toggle(), ago(), useApp(), useCurrentGame() (+12 more)

### Community 144 - "Community 144"
Cohesion: 0.17
Nodes (17): add(), detect(), GameArgs, GameCmd, Add, Detect, Info, List (+9 more)

### Community 145 - "Community 145"
Cohesion: 0.15
Nodes (20): argOf(), bbcodePlain(), blank(), Br, COLOR, decodeEntities(), El, KNOWN (+12 more)

### Community 146 - "Community 146"
Cohesion: 0.13
Nodes (16): MAX_DEPTH, MAX_INPUT, ExternalLink(), LINK_CLASS, MediaLink(), webUrl(), ALLOWED, components (+8 more)

### Community 147 - "Community 147"
Cohesion: 0.19
Nodes (11): games_key(), has_table(), libraries(), open(), PATH_COLUMNS, PrefixMap, replace_link(), Retarget (+3 more)

### Community 148 - "Community 148"
Cohesion: 0.19
Nodes (12): cache_dir(), _cdn_ok(), cmd_github(), cmd_nexus(), describe(), detect_format(), download(), _github_json() (+4 more)

### Community 149 - "Community 149"
Cohesion: 0.10
Nodes (20): dependencies, @fontsource-variable/geist@5.3.0, @fontsource-variable/geist-mono@5.3.0, @phosphor-icons/react@2.1.10, react@19.3.1, react-dom@19.3.1, react-markdown@10.1.0, remark-gfm@4.0.1 (+12 more)

### Community 150 - "Community 150"
Cohesion: 0.19
Nodes (12): external(), forward_to_gui(), handle(), notify(), NxmArgs, print_help(), program_path(), register() (+4 more)

### Community 151 - "Community 151"
Cohesion: 0.30
Nodes (14): PREVIEW_TTL, age(), DAY, expired_preview_and_scratch_go_live_one_survives(), Fx, fx(), orphan_read_only_scratch_is_removed_without_following_links(), proton_temp_cleanup_touches_only_smapi_installer_dirs() (+6 more)

### Community 152 - "Community 152"
Cohesion: 0.16
Nodes (7): init(), NOISY_TARGETS, target_cap(), http_and_tls_targets_are_capped_at_warn(), init(), NOISY_TARGETS, target_cap()

### Community 153 - "Community 153"
Cohesion: 0.16
Nodes (10): CP1252_HIGH, decode(), decodes_utf16_and_cp1252(), El, local(), parse(), predefined(), start() (+2 more)

### Community 154 - "Community 154"
Cohesion: 0.19
Nodes (13): bad(), collection_forms(), id(), MAX_NXM_LEN, mod_link_with_key(), mod_link_without_key(), NxmLink, Collection (+5 more)

### Community 155 - "Community 155"
Cohesion: 0.29
Nodes (16): bat_args_are_checked_and_cmd_line_is_exact(), Env, env(), env_probe_inner(), flatpak_vessel_refuses_proton_runs(), host_and_kind_must_fit(), hostile_game_path_is_one_argv_element(), logs_capture_both_streams_and_spawn_is_reported_first() (+8 more)

### Community 156 - "Community 156"
Cohesion: 0.19
Nodes (12): APPIMAGE_DIRS, find_appimage(), gui(), or_self(), print_status(), restart_hint(), run(), SteamArgs (+4 more)

### Community 158 - "Community 158"
Cohesion: 0.16
Nodes (11): SharedStore, BATCH_SIZE, CachedRequirements, fatal(), lock(), mod_from_url(), refresh_requirements(), Requirement (+3 more)

### Community 160 - "Community 160"
Cohesion: 0.24
Nodes (15): CtaButton(), Thumb, bytes(), dateTime(), plural(), rtf, DraggableRow(), Fact() (+7 more)

### Community 162 - "Community 162"
Cohesion: 0.12
Nodes (12): LayoutProfile, .ALL, Bannerlord, Bepinex, Bethesda, Cyberpunk, Generic, Melonloader (+4 more)

### Community 163 - "Community 163"
Cohesion: 0.26
Nodes (8): builtin_order(), emit(), load(), main(), open_prs(), remote_branches(), rerun_branch(), run()

### Community 165 - "Community 165"
Cohesion: 0.33
Nodes (14): App(), LoadingShell(), VIEWS, Button(), EmptyState(), ErrorNote(), PageHeader(), quick (+6 more)

### Community 166 - "Community 166"
Cohesion: 0.21
Nodes (3): git(), Picker, toml()

### Community 167 - "Community 167"
Cohesion: 0.21
Nodes (16): Add full game support prompt, Loader routes (distribution routes per loader), Re-run mode (RERUN=1), Research note template (docs/game-support/DOMAIN.md), Cyberpunk 2077 game support note, ArchiveXL / TweakXL / Codeware (RED4ext plugins, not loaders), Cyber Engine Tweaks loader (Nexus 107), Cyberpunk load order (archive/pc/mod ASCII, no plugin list) (+8 more)

### Community 168 - "Community 168"
Cohesion: 0.27
Nodes (5): deploy_list_purge_manage_plugins_txt(), Env, GAME, plugins_command_rejects_non_bethesda_game(), plugins_masterlist_downloads_from_override_url()

### Community 170 - "Community 170"
Cohesion: 0.20
Nodes (16): Legacy modman v3.2.0 behaviour spec, Legacy ambiguities A1-A12, Legacy auto-update state machine, Legacy collections install and load ordering, Legacy command inventory and aliases, Legacy deploy, backup, purge, verify algorithm, Legacy deps (requirements) analysis and ack, Legacy FOMOD modes and choice storage (+8 more)

### Community 171 - "Community 171"
Cohesion: 0.12
Nodes (15): compilerOptions, isolatedModules, jsx, lib, module, moduleResolution, noEmit, noFallthroughCasesInSwitch (+7 more)

### Community 172 - "Community 172"
Cohesion: 0.13
Nodes (14): compilerOptions, isolatedModules, jsx, lib, module, moduleResolution, noEmit, noUnusedLocals (+6 more)

### Community 173 - "Community 173"
Cohesion: 0.23
Nodes (12): dead_soon(), group_alive(), group_alive_ignores_a_reused_leader_pid(), kill_group(), kill_stale(), kill_stale_reaps_a_group_whose_leader_died(), kill_stale_run(), proc_pgrp() (+4 more)

### Community 174 - "Community 174"
Cohesion: 0.33
Nodes (7): apply(), dry_run(), dry_run_msgs(), env_for(), list_quarantines(), MAX_LISTED, restore_quarantine()

### Community 175 - "Community 175"
Cohesion: 0.13
Nodes (13): UnknownSetting, GameSetting, .ALL, Appid, Folder, Method, Name, .NAMES (+5 more)

### Community 176 - "Community 176"
Cohesion: 0.23
Nodes (15): Mod loaders (SMAPI) and security model, Adopt manual install and junk cleanup, Touch-scope backups and rollback, Loader and launch error codes, Flatpak limits for loaders, Installer security model (clean env, no sandbox, timeout), Steam launch options (copy/acknowledge or Apply), SMAPI native vs Proton runtime detection (+7 more)

### Community 177 - "Community 177"
Cohesion: 0.35
Nodes (14): make_handler(), _api_get(), _body(), do_GET(), do_POST(), _error(), _events(), _game() (+6 more)

### Community 178 - "Community 178"
Cohesion: 0.21
Nodes (6): Boundary, Props, recoverQueries(), refocusView(), RootBoundary(), ViewBoundary()

### Community 179 - "Community 179"
Cohesion: 0.18
Nodes (8): LibraryState, Conflict, Done, Offline, Pending, Marker, MARKER_VERSION, Sources

### Community 180 - "Community 180"
Cohesion: 0.29
Nodes (8): conflict_data(), ConflictData, FileConflict, ModInfo, plugin_at(), plugin_at_only_matches_top_of_deploy_folder(), PluginEntry, is_plugin_name()

### Community 181 - "Community 181"
Cohesion: 0.26
Nodes (10): an_inherited_appimage_variable_does_not_replace_the_cli(), appimage_path_is_used_for_exec_start(), enable_status_disable_manage_user_timer_units(), env(), flatpak_sandbox_refuses_enable_status_and_run(), forced_run_redeploys_and_logs_the_run(), no_scheduler_flag(), run_without_games_is_quiet() (+2 more)

### Community 182 - "Community 182"
Cohesion: 0.20
Nodes (7): list(), PluginsArgs, PluginsCommand, List, Masterlist, Sort, run()

### Community 183 - "Community 183"
Cohesion: 0.14
Nodes (12): GameOutcome, Acted, Failed, Nothing, Skip, BackOff, Baseline, FolderMissing (+4 more)

### Community 184 - "Community 184"
Cohesion: 0.23
Nodes (5): contains(), GameProbe, NeverRunning, ProcProbe, Running

### Community 185 - "Community 185"
Cohesion: 0.24
Nodes (8): codes_are_unique_and_match_fixture(), FIXTURE, io(), launch_samples(), LaunchError, LoaderError, samples(), store()

### Community 186 - "Community 186"
Cohesion: 0.18
Nodes (9): CommandPlan, FakeOp, Chmod, Remove, RemoveAll, Rename, Symlink, Write (+1 more)

### Community 187 - "Community 187"
Cohesion: 0.28
Nodes (8): cells(), check(), check_routes(), field_set(), loader_entry(), main(), norm(), section()

### Community 188 - "Community 188"
Cohesion: 0.21
Nodes (7): autoupdate_enable(), loader_run(), log_tail_streams_whole_redacted_lines(), LogTail, nexus_secret(), read_from(), sink()

### Community 189 - "Community 189"
Cohesion: 0.39
Nodes (6): fail!, files_for(), list(), loot_base(), masterlist(), sort()

### Community 190 - "Community 190"
Cohesion: 0.23
Nodes (5): copy_preserves_mode_and_mtime(), explicit_method_is_used_with_copy_fallback(), forced_hardlink_failure_records_fallback_method_per_file(), Fx, verify_detects_broken_symlink()

### Community 193 - "Community 193"
Cohesion: 0.20
Nodes (10): BOLD, CYAN, DIM, GREEN, human_size(), json_strings_are_escaped(), RED, RESET (+2 more)

### Community 194 - "Community 194"
Cohesion: 0.33
Nodes (6): ignore_missing(), run_ok(), RunOutcome, RunRequest, Spawned, InstallerSpec

### Community 195 - "Community 195"
Cohesion: 0.20
Nodes (7): LayoutOptions, non_utf8_names_are_refused_not_mangled(), normalize_layout(), placer_refuses_parent_components(), sanitize_name(), sanitize_name_matches_legacy(), VariantChooser

### Community 196 - "Community 196"
Cohesion: 0.25
Nodes (8): EntryType, Dir, File, Hardlink, Special, Symlink, sevenz_entry_type(), type_from_mode()

### Community 198 - "Community 198"
Cohesion: 0.22
Nodes (7): FomodMode, Ask, Auto, Collection, Default, None, Reuse

### Community 200 - "Community 200"
Cohesion: 0.31
Nodes (9): Archive test fixtures README, Adversarial archive fixtures (traversal, absolute, symlink, hardlink), libarchive RAR test vectors, T02 spikes: external dependency decisions, Pinned dependency table and deny.toml bans, libloot 0.29.7 (pure Rust load-order sorting), RAR via compress-tools / libarchive, reqwest with rustls + ring (no aws-lc) (+1 more)

### Community 201 - "Community 201"
Cohesion: 0.28
Nodes (9): Legacy Nexus API, rate limit, downloads, Legacy nxm register and handle flow, Keyring with 3s probe and file fallback, nxm routing: CLI handler forwards to GUI over D-Bus single-instance, Migrated integrations (keyring, timer, nxm handler, Steam shortcut), Migration lock and modman-running check, Resumable migration (migration.json), Ignored MODMAN_* environment variables (+1 more)

### Community 202 - "Community 202"
Cohesion: 0.25
Nodes (7): /add-game-support command, check_game_support_note.py checker, inspect_archive.py helper, Interactive game-support flow (no commit/push/PR), nexus_mod_info.py helper, pick_game_support_target.py picker, scripts/run-game-support.sh (unattended runner)

### Community 203 - "Community 203"
Cohesion: 0.32
Nodes (6): blocking(), exe(), log_tail(), open_url(), steam_shortcut_set(), steam_shortcut_status()

### Community 204 - "Community 204"
Cohesion: 0.29
Nodes (7): Installer redirect (Nexus installer-only page -> GitHub release), Loader definition fields (nexus_mod_id, required_paths, github, aliases), MelonLoader loader (Nexus 1196, installer-only), MelonLoader.x64.zip GitHub release asset, Mono vs IL2CPP build choice (not expressible), melonwiki.xyz / GitHub off-site requirement aliases, SMAPI loader (built-in Rust plugin, Nexus 2400)

### Community 205 - "Community 205"
Cohesion: 0.48
Nodes (5): loader_env(), loader_preview(), loader_preview_reopen(), preview_view(), PreviewView

### Community 206 - "Community 206"
Cohesion: 0.71
Nodes (6): add_appimage(), add_flatpak(), add_package(), die(), pick(), add-to-steam.sh script

### Community 207 - "Community 207"
Cohesion: 0.33
Nodes (6): scripts, build, dev, tauri, test, typecheck

### Community 208 - "Community 208"
Cohesion: 0.33
Nodes (5): description, identifier, permissions, $schema, windows

### Community 209 - "Community 209"
Cohesion: 0.47
Nodes (4): dmabuf_default(), dmabuf_renderer_is_disabled_only_when_unset(), DMABUF_VAR, main()

### Community 211 - "Community 211"
Cohesion: 0.33
Nodes (6): AppError, Autoupdate, Collection, Core, Msg, Sql

### Community 213 - "Community 213"
Cohesion: 0.33
Nodes (5): LayoutOutcome, ModRoot, Deploy, Game, Run

### Community 214 - "Community 214"
Cohesion: 0.60
Nodes (3): Selection, mount(), useSelection()

### Community 215 - "Community 215"
Cohesion: 0.40
Nodes (5): ProfileCmd, Delete, List, Load, Save

### Community 216 - "Community 216"
Cohesion: 0.60
Nodes (4): acf(), detect_installed(), detect_installed_lists_non_preset_games_from_every_library(), DetectedGame

### Community 217 - "Community 217"
Cohesion: 0.40
Nodes (5): InstallerTarget, TargetKind, BashScript, BatScript, Exe

### Community 218 - "Community 218"
Cohesion: 0.40
Nodes (5): Moved, Absent, Conflict, Copied, Renamed

### Community 219 - "Community 219"
Cohesion: 0.60
Nodes (5): T1 findings: Steam games missing from Games, Detected lacks appid (art lookup gap G1), Fix: list_installed_apps excluding tools/runtimes, Root cause: preset-only filter in detect_installed, Regression test fixture for Steam detection

### Community 220 - "Community 220"
Cohesion: 0.50
Nodes (4): Arg, GamePath, GamePathWin, Lit

### Community 221 - "Community 221"
Cohesion: 0.50
Nodes (4): T6 review: browse renderer, open_url, steam_art, capabilities, open_url command (http/https only, Rust-parsed), Safe BBCode/Markdown description renderer, steam_art command (appid-only fetch)

### Community 222 - "Community 222"
Cohesion: 0.50
Nodes (4): T3 validation log, D1: why cargo test --workspace stalls, Slow linking on compressed btrfs target dir, Orphaned pgrep polling shells

### Community 225 - "Community 225"
Cohesion: 0.67
Nodes (3): Bulk selection bar (Mods), Danger confirm dialog (PromptDialog), Pagination toolbar (Browse and Mods load order)

### Community 226 - "Community 226"
Cohesion: 0.67
Nodes (3): vessel-cli, vessel-core, vessel-desktop

## Knowledge Gaps
- **552 isolated node(s):** `vessel-desktop`, `Started`, `Finished`, `Failed`, `Accepted` (+547 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 2051 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **26 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Store` connect `Loader Archive Cache` to `Community 132`, `Community 133`, `Declarative Loaders`, `Loader Errors`, `Launch & Apply`, `Deploy Confinement`, `Mods Model`, `SMAPI Support`, `Nexus Client`, `CLI App`, `Community 142`, `Collection CLI Tests`, `Loader Adopt/Preview`, `Community 137`, `Autoupdate`, `Community 151`, `Autoupdate Tests`, `Games & Settings`, `Collections`, `Requirements`, `Community 158`, `Deploy Tests`, `Download Core`, `Community 161`, `Abort Dialogs`, `Restage`, `Restore`, `Community 168`, `Community 40`, `Community 42`, `Community 46`, `Community 47`, `Community 49`, `Community 50`, `Community 51`, `Community 56`, `Community 58`, `Community 60`, `Community 61`, `Community 190`, `Community 62`, `Community 68`, `Community 197`, `Community 72`, `Community 73`, `Community 77`, `Community 78`, `Community 85`, `Community 88`, `Community 91`, `Community 95`, `Community 98`, `Community 105`, `Community 109`, `Community 110`, `Community 126`?**
  _High betweenness centrality (0.056) - this node is a cross-community bridge._
- **Are the 8 inferred relationships involving `Store` (e.g. with `P` and `Fx`) actually correct?**
  _`Store` has 8 INFERRED edges - model-reasoned connections that need verification._
- **What connects `vessel-desktop`, `Started`, `Finished` to the rest of the system?**
  _552 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Frontend App Tests` be split into smaller, more focused modules?**
  _Cohesion score 0.02736318407960199 - nodes in this community are weakly interconnected._
- **Why does `LoaderError` connect `Loader Errors` to `Community 194`, `Tauri Commands`, `Community 70`, `Loader Archive Cache`, `Declarative Loaders`, `Community 73`, `Launch & Apply`, `Community 105`, `Community 136`, `Community 173`, `SMAPI Support`, `Loader Adopt/Preview`, `Community 117`, `Community 185`, `Community 58`, `Community 92`?**
  _High betweenness centrality (0.036) - this node is a cross-community bridge._
- **Are the 2 inferred relationships involving `Cx` (e.g. with `find_loader_junk()` and `remove_loader_junk()`) actually correct?**
  _`Cx` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Should `Loader Cards UI` be split into smaller, more focused modules?**
  _Cohesion score 0.01801938987868636 - nodes in this community are weakly interconnected._