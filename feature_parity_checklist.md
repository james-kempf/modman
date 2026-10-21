# Feature-Parity Checklist: legacy `modman` v3.2.0 -> Rust rewrite

Source: the legacy `modman` help text and command dispatch, plus the view list in `web/src/views`. Behaviour details were audited in `docs/plan/20261020-rust-migration/legacy_spec.md` (T01).

**Status values:** `Port` = same behaviour, `Change` = ported with a deliberate difference (stated in Notes), `Drop` = not carried over, `Later` = after v1, `New` = does not exist in legacy.
**Milestones** refer to §8 of `linux_mod_manager_tech_stack.md` (M1..M7).
**v1 reference game:** Skyrim Special Edition.

**Proof column** (filled by the T17 gate, 2026-10-21, at `1ccd93d`; re-gated at `d7d14fd`): test names are Rust `#[test]` functions or vitest cases. Prefixes: `cli/<file>` = `crates/modman-cli/tests/<file>.rs`; `core/<file>` = `crates/modman-core/tests/<file>.rs`; `core::<module>` = unit tests in `crates/modman-core/src`; `desktop::<module>` = unit tests in `apps/desktop/src-tauri/src`; `vitest <file>` = `apps/desktop/src/**`. "T17 E2E" = the bethesda end-to-end scenario recorded in `docs/plan/20261020-rust-migration/t17_report.md`. "T16 flow" = the Playwright flows in `docs/plan/20261020-rust-migration/t16/flows.mjs` (rerun by T17: 30/30; committed rerun evidence `t16/flows.json` 30/30 in `d7d14fd`). All named tests passed in the T17 run of `cargo test --workspace` and `npx vitest run`.

---

## 1. Global CLI behaviour

| Feature | Legacy | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `-g/--game <domain>` per-command game override | yes | Port | M1 | `--game=<d>` form also accepted. | cli/cli `game_flag_overrides_active_game_in_every_spelling`; core::games `resolve_game_order_is_override_active_then_single` |
| `-y/--yes` assume yes | yes | Port | M1 | | cli/cli `yes_flag_answers_prompts` |
| `--no-color`, `--debug` | yes | Port | M1 | Also honours `NO_COLOR`. | cli/cli `colour_is_forced_only_without_no_color_settings`, `debug_flag_prints_debug_lines_on_stderr`, `double_dash_ends_option_parsing` |
| `help`, `version` | yes | Port | M1 | | cli/cli `help_exits_zero_in_every_form`, `version_exits_zero_in_every_form`, `unknown_command_exits_non_zero`; T17 `--help` diff |
| Mod reference forms: `#12` position, full id, exact name, Nexus mod id, unique name part | yes | Port | M3 | All five kept; ambiguity errors list matches. | core::mods `resolves_position_tier_first`, `exact_id_beats_nexus_id_and_substring`, `exact_name_is_case_insensitive_and_beats_substring`, `nexus_mod_id_tier`, `unique_substring_resolves`, `ambiguous_substring_lists_every_match`; cli/mods `mod_reference_forms_and_ambiguity_errors_from_the_cli` |
| Command aliases (`i`, `ls`, `rm`, `dl`, `st`, ...) | yes | Port | M1 | All legacy aliases and short flags resolve (`deploy -n` added in T17F `d7d14fd`). | cli/cli `every_legacy_alias_resolves_to_its_command`, `deploy_short_n_is_dry_run`; T17 `--help` diff (report §3) |
| Lock file preventing concurrent runs | `modman.lock` (flock) | Change | M2 | Difference: no global flock; SQLite WAL for state plus an exclusive deploy lock for deploy/purge. | core::store `deploy_lock_is_exclusive_until_dropped`, `file_db_uses_wal_and_foreign_keys`; core/deploy `deploy_and_purge_refused_while_lock_held`; cli/deploy `deploy_refused_while_lock_held`; core::autoupdate `concurrent_run_returns_at_once` |
| Required external tools (`curl jq python3 file flock 7z bsdtar unrar`) | yes | Change | M3 | Difference: no external tools. HTTP, JSON, FOMOD, zip/7z/tar are in Rust; RAR goes through system `libarchive` (compress-tools; bundled in the AppImage). A missing libarchive is a typed error. | core::archive `extracts_zip_fixture`, `extracts_7z_fixture`, `extracts_rar_fixture`, `extracts_tar_fixture`, `missing_libarchive_is_a_typed_error_naming_it` |
| Data and state location | `~/.config/nexus-mod-manager`, data `~/nexus-mods` | Change | M1 | Difference: XDG dirs (`~/.local/share/modman`, `~/.local/state/modman/modman.log`, `~/.config/modman`). Legacy state is not migrated; a warning is printed when it is found. | core::paths `files_live_in_xdg_dirs`, `legacy_state_detected_only_when_present`; cli/cli `warns_when_legacy_state_exists` |

## 2. Setup and accounts

| Command | Legacy behaviour | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `login` | Nexus API key + first game setup | Change | M3 | Difference: key stored in the Secret Service keyring with a 0600 file fallback (3 s probe); key never accepted as an argument and never printed. Premium detection kept. OAuth2/SSO is Later. | cli/nexus `login_reads_key_from_stdin_stores_it_and_never_prints_it`, `login_never_accepts_the_key_as_an_argument`, `login_rejected_key_is_not_stored`; core/nexus `validate_key_success_reports_premium_and_sends_headers`, `keyring_unavailable_falls_back_to_0600_file_in_0700_dir`, `keyring_probe_timeout_triggers_fallback`, `keyring_available_is_used_and_removes_file_copy`; T17 E2E |
| `logout` | Forget API key | Port | M3 | Warns when `NEXUS_API_KEY` is still set. | cli/nexus `login_reads_key_from_stdin_stores_it_and_never_prints_it` (logout step) |
| `game detect` | List supported Steam games installed | Port | M1 | `libraryfolders.vdf` + `appmanifest_*.acf`, including SD cards. | cli/cli `game_detect_lists_installed_presets`; core::games::steam `parses_fixture_libraryfolders_with_sd_card`, `detects_app_on_sd_card_with_prefix_and_buildid`, `prefix_falls_back_to_other_library`; T17 E2E |
| `game add <domain> [path]` | Set up a game, folder auto-detected | Port | M1 | Manual path kept as override; `--subdir`, `--profile`. | cli/cli `game_add_preset_then_list_info_use`, `game_add_detects_steam_folder_on_sd_card`, `game_add_validates_domain_path_and_profile`, `game_add_custom_domain_uses_generic_defaults_and_flags`, `game_add_experimental_preset_warns`; cli/nexus `game_add_checks_domain_on_nexus_when_logged_in`; T17 E2E |
| `game list` / `use <domain>` | List games, switch active game | Port | M1 | Aliases `ls`, `switch`, `select`. | cli/cli `game_add_preset_then_list_info_use`, `every_legacy_alias_resolves_to_its_command` |
| `game info` / `set <key> <value>` | Show/change folder, subdir, method | Port | M1 | Exactly the nine legacy keys (folder, subdir, profile, appid, method, prefix, plugins-file, paks-dir, name). | cli/cli `game_set_accepts_exactly_the_nine_legacy_keys`, `game_set_folder_refused_while_deployed`; core::games `set_values_are_validated_and_normalised`, `subdir_and_paks_dir_settings_refuse_escaping_paths` |
| `game remove <domain>` | Forget a game (purge first) | Port | M2 | Refused while a deployment is active. | cli/cli `game_remove_refuses_while_deployment_active`, `game_remove_delete_data_removes_game_data`; core::games `remove_refuses_while_deployed_or_backups_held` |
| `config [<key> [<value>]]` | Global settings | Change | M1 | Same nine keys. Differences: `deploy-method` also accepts `reflink`; `data-dir` defaults to XDG; the `fomod` key is now read as the install default (legacy A2: dead key); `jobs` is accepted but unused (downloads are sequential, see §3). | core::app::install `fomod_config_key_sets_install_mode_unless_flag_given`; cli/cli `config_lists_the_nine_keys_with_defaults`, `config_sets_each_key_and_persists`, `config_unknown_key_or_bad_value_exits_non_zero`; core::config `accepts_exactly_the_nine_documented_keys`, `defaults_match_legacy`, `validation_per_key` |
| `nxm register / unregister` | Register `nxm://` handler | Change | M3 | Difference: writes the `.desktop` entry and edits `mimeapps.list` directly (legacy used `xdg-mime`); Exec quoting proven injection-safe; uses `$APPIMAGE` only when running from the mount; `nxm <sub> --help` prints help and changes nothing. | cli/nexus `nxm_register_writes_desktop_entry_and_mimeapps_then_unregister_removes_both`, `nxm_subcommand_help_prints_help_and_changes_nothing`, `nxm_register_uses_appimage_path`, `nxm_register_ignores_an_inherited_appimage_variable`; modman-cli `commands::nxm::tests::writing_through_a_symlinked_mimeapps_keeps_the_link`, `desktop_entry_matches_the_packaged_one`; core::ipc `special_characters_cannot_break_out_of_the_quotes` |
| `nxm <url>` | Queue incoming link | Change | M3 | Difference: forwards to a running GUI over D-Bus single-instance; without a GUI the link is queued in SQLite and processed inline (legacy `nxm-queue` dir). | cli/nexus `nxm_link_without_gui_is_queued_and_installed_inline`, `bare_nxm_link_routes_to_handler_and_bad_links_fail`, `nxm_collection_link_without_login_fails_and_marks_row_failed`; desktop::nxm `a_forwarded_valid_url_is_parsed_queued_and_redacted`, `forwarded_malformed_or_foreign_urls_are_rejected_without_enqueue`; core::download::nxm `malformed_and_foreign_input_rejected` |
| Per-game config files (`games/<domain>.cfg`) | yes | Change | M1 | Difference: SQLite `games` table. | core::games `save_and_load_roundtrip`; core::store `migrations_create_every_required_table_on_empty_db` |
| v2 -> v3 config migration | yes | Drop | - | Not implemented; legacy state only triggers a warning. | grep: no migration code (T17 report §2); cli/cli `warns_when_legacy_state_exists` |

## 3. Mods

| Command | Legacy behaviour | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `install <id\|url\|nxm\|file>` | Nexus id, URL, nxm link or local archive | Port | M3 | | cli/mods `install_local_archive_stages_files_and_lists_mod`, `install_rejects_unknown_targets_and_routes_collections`; cli/nexus `install_by_nexus_id_with_premium_link`, `install_by_id_requires_login`; core::app::install `targets_parse_like_legacy`; T17 E2E |
| `install --file <id>` | Pick a specific file | Port | M3 | | cli/nexus `install_by_nexus_id_with_premium_link` (`--file 12`, `-f 12`); core::app::install `file_rows_default_to_the_primary_file` |
| `install --fomod ask\|default` | FOMOD behaviour | Change | M5 | Difference: an explicit `--fomod ask` without a TTY reads answers from stdin (legacy fell back to defaults); `none`/`reuse` kept. | cli/mods `install_fomod_ask_reads_scripted_answers`, `install_fomod_default_installs_default_option_and_info_shows_choices`, `install_without_tty_uses_defaults_and_fomod_none_uses_plain_layout`; core::fomod `mode_resolution_matches_legacy_table` |
| `install --deploy` | Deploy afterwards | Port | M3 | | cli/mods `install_aliases_name_flag_and_deploy_flag` |
| `download <id\|url\|collection>` | Download without installing | Port | M3 | | cli/nexus `download_saves_archive_without_installing`; cli/collections `download_only_fetches_archives_without_installing` |
| `list [text] [-e\|-d] [-c <slug>]` | Priority order; filter enabled/disabled/collection | Port | M3 | | cli/mods `list_filters_by_state_text_and_collection` |
| `info <mod>` | Details, FOMOD choices, conflicts | Port | M3 | | cli/mods `install_fomod_default_installs_default_option_and_info_shows_choices` |
| `enable / disable <mod...>` (`--all`, `--collection`) | Toggle | Port | M3 | | cli/mods `enable_disable_by_ref_all_and_collection`; core::mods `enable_disable_and_change_tracking` |
| `order <mod> <where>` (`#`, first, last, up, down, before X, after X) | Priority | Port | M3 | | cli/mods `order_variants_and_aliases`; core::mods `order_specs_parse`, `move_mod_follows_legacy_positions_and_clamps` |
| `uninstall <mod...>` (`--delete-archive`) | Remove | Port | M3 | `--purge-archive` alias kept. | cli/mods `uninstall_aliases_and_delete_archive`; core::mods `remove_deletes_staging_and_only_unshared_archives_in_downloads` |
| `reinstall <mod> [--ask]` | Re-extract; `--ask` re-runs FOMOD | Port | M5 | `--fomod` is validated (legacy did not). | cli/mods `reinstall_reextracts_and_keeps_state`, `reinstall_reuses_choices_and_ask_reprompts`; core::mods::install `fomod_ask_then_reuse_replays_stored_choices_on_reinstall` |
| `conflicts [mod] [--files]` | Which mods override each other | Port | M3 | GUI conflict view in M6. | cli/mods `conflicts_lists_pairs_filter_and_files`; core/deploy `later_priority_mod_wins_each_contested_path_and_conflicts_are_listed`; core/loadorder `conflict_data_reports_file_and_plugin_conflicts` |
| `deps [mod] [--refresh]` | Required mods + external tools (UE4SS, SKSE...) | Port | M3 | Nexus requirements (7-day cache) + legacy tool patterns. | cli/mods `deps_reports_external_tools_and_ack_unack_persist`, `deps_finds_tool_files_in_game_folder`; core::requirements `tool_patterns_match_legacy_regexes`, `report_groups_tools_nexus_and_external_with_acks`; core/nexus `requirements_cached_with_ttl_and_availability_checked` |
| `deps ack / unack <name>` | Mark external tool installed by hand | Port | M3 | | cli/mods `deps_reports_external_tools_and_ack_unack_persist` |
| `updates [--install] [--all]` | Check Nexus mods for newer files | Port | M3 | | cli/nexus `updates_lists_and_installs_newer_files`; core::nexus::updates `latest_file_follows_chain_and_skips_archived`; core/nexus `update_check_uses_cache_and_updated_list` |
| `search <text>` | Search Nexus (GraphQL) | Port | M3 | | cli/nexus `search_prints_results_and_json`; core/nexus `search_sends_no_adult_flag_and_collection_query_does` |
| `profile list/save/load/delete` | Save/restore load order + enabled state | Port | M3 | SQLite `profiles` table. | cli/mods `profile_save_load_roundtrip_and_aliases`; core::mods::profiles `save_load_roundtrip_restores_order_and_enabled_state` |
| Parallel Premium downloads (`jobs`) | yes | Change | M3 | Difference: downloads run one at a time; `jobs` is kept as a config key but has no effect; its help reads "reserved; downloads run one at a time" and the Premium login message no longer promises parallel downloads (T17F `d7d14fd`). | grep: `jobs` is read only by `core::config` (T17 report §2) |
| Per-mod staging folder `<data-dir>/<game>/mods/<id>` | yes | Change | M2 | Difference: staging lives per Steam library so hardlinks stay on one filesystem (tech stack §4.1). | core/deploy `staging_lives_per_steam_library`, `cross_device_staging_skips_hardlink_and_reflink`; core::deploy `library_is_derived_only_from_steamapps_common` |
| Archive formats zip / 7z / rar | yes | Port | M3 | RAR via libarchive. Entry-path, link and size checks added. | core::archive `extracts_zip_fixture`, `extracts_7z_fixture`, `extracts_rar_fixture`, `zip_rejects_dotdot_and_absolute_entries`, `rar_rejects_symlink_entries`, `extraction_stops_at_file_count_and_size_caps` |
| Case-insensitive folder matching | yes | Port | M2 | | core::staging `case_insensitive_merge_into_one_canonical_directory`, `merge_into_existing_output_keeps_existing_spelling`; core::deploy::plan `resolver_reuses_existing_spelling_and_records_new_dirs_once`; core::fomod `file_dependency_checks_the_deploy_folder_case_insensitively` |
| Layout profiles: bethesda, cyberpunk, stardew, bannerlord, bepinex, melonloader, witcher3, ue, generic | yes | Port | M2 | TOML game definitions (`crates/modman-core/definitions`). Only `bethesda` for Skyrim SE is E2E-tested in v1; other presets are unit-tested and labelled experimental. | core::staging `bethesda_strips_wrapper_and_data_and_drops_docs`, `cyberpunk_folders_by_content`, `stardew_manifest_at_root_and_nested`, `bannerlord_submodule_marker`, `bepinex_places_into_bepinex_tree`, `melonloader_dlls_to_mods`, `witcher3_mod_and_dlc_folders`, `ue_flattens_paks_into_paks_dir`, `generic_keeps_structure_and_docs`; core::games::definitions `presets_match_legacy_table`, `one_builtin_per_legacy_preset`, `every_other_preset_is_experimental`, `skyrim_se_is_the_tested_reference_game` |

## 4. Collections

| Command | Legacy behaviour | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `collection install <url\|slug>` | Download + install with FOMOD choices and load-order rules | Port | M5 | | cli/collections `collection_install_applies_choices_rules_and_skips_optional`, `install_command_routes_collections_with_optional_all_and_revision`; core::collections `rule_order_applies_before_and_after_rules`, `plan_dedupes_by_md5_then_mod_and_file_id` |
| `--optional all\|none\|ask` | Optional mods | Port | M5 | | cli/collections `collection_install_applies_choices_rules_and_skips_optional`, `install_command_routes_collections_with_optional_all_and_revision`; core::collections `select_keeps_required_and_picked_optional`, `parse_selection_matches_legacy` |
| `--revision N`, `--deploy` | Pin revision; deploy after | Port | M5 | | cli/collections `install_command_routes_collections_with_optional_all_and_revision`, `nxm_collection_link_installs_the_pinned_revision`; core/collections `fetch_revision_pins_revision_and_sends_adult_setting`; modman-cli `commands::collection::tests::passthrough_parses_collection_flags` |
| `collection list / info` | Installed; details and update availability | Port | M5 | | cli/collections `collection_info_and_bad_references`; cli/cli `former_stub_commands_are_implemented_and_bare_nxm_link_is_routed` |
| `collection update <slug>` | Newest revision, removes dropped mods | Change | M5 | Difference: the revision query sends the `adult` setting (legacy omitted it on update). | cli/collections `collection_update_removes_dropped_mods_then_remove_keep_mods_keeps_them`; core::collections `dropped_members_skips_kept_and_still_listed_mods`; core/collections `fetch_revision_pins_revision_and_sends_adult_setting` |
| `collection remove <slug>` (`--keep-mods`) | Uninstall | Port | M5 | | cli/collections `collection_remove_uninstalls_exclusive_mods`, `collection_update_removes_dropped_mods_then_remove_keep_mods_keeps_them`; core::collections `remove_uninstalls_exclusive_mods_and_keep_mods_keeps_them` |
| `collection download` | Download only | Port | M5 | | cli/collections `download_only_fetches_archives_without_installing` |
| `nxm://.../collections/<slug>/revisions/<n>` input | yes | Port | M5 | | cli/collections `nxm_collection_link_installs_the_pinned_revision`; core::download::nxm `collection_forms`; core::collections `parse_ref_accepts_legacy_forms_and_rejects_others` |
| FOMOD choices from collection (`collection` / `reuse` / `ask` / `default` modes) | yes | Port | M5 | Choices stored Vortex-style. | core::mods::install `fomod_collection_mode_uses_given_choices_and_never_asks`, `fomod_ask_then_reuse_replays_stored_choices_on_reinstall`, `fomod_none_mode_skips_installer_and_clears_choices`; core::fomod `choices_round_trip_in_vortex_format`, `choices_mode_replays_by_name_then_idx_and_hides_steps` |

## 5. Deployment

| Command | Legacy behaviour | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `deploy [--dry-run] [--method m]` | Link enabled mods into the game | Port | M2 | `-n` short form of `--dry-run` kept. | cli/cli `deploy_short_n_is_dry_run`; cli/deploy `deploy_verify_status_purge_roundtrip_restores_game`, `deploy_dry_run_changes_nothing`, `deploy_method_flag_is_recorded_and_validated`; core/deploy `dry_run_changes_nothing_on_disk`; T17 E2E |
| Methods hardlink / symlink / copy (reflink) | yes | Change | M2 | Difference: `reflink` method added; automatic fallback chain (hardlink -> reflink -> copy, never symlink in auto) with the method recorded per file. | core/deploy `method_resolution_precedence`, `forced_hardlink_failure_records_fallback_method_per_file`, `reflink_is_used_when_hardlink_fails_on_a_reflink_filesystem`, `explicit_method_is_used_with_copy_fallback`, `copy_preserves_mode_and_mtime`; core::deploy `chains_end_in_copy` |
| All-or-nothing deploy | partial (per-file) | Change | M2 | Difference: a failed deploy rolls back every file it placed; deploy is confined to the game folder/prefix (no symlink escape). | core/deploy `real_failure_mid_deploy_rolls_back_immediately`, `path_conflict_with_existing_directory_is_refused_before_changes`, `deploy_refuses_symlinked_folder_leading_outside_game_dir`, `folder_swapped_for_outside_symlink_after_preflight_is_refused_at_placement` |
| Back up replaced game files | yes | Port | M2 | Content-addressed backup store. | core/deploy `backups_are_content_addressed_and_released_after_purge`, `recovery_sweeps_orphaned_backup_blobs_but_keeps_held_ones`; core::deploy::backup `store_dedups_and_restore_roundtrips_mode_mtime_and_symlinks` |
| `purge` / `undeploy` | Remove deployed files, restore originals | Port | M2 | Game folder and Proton prefix restored byte-identical (T17 E2E re-gate at `d7d14fd`). The legacy no-op `--legacy` flag (v2 manifest, A3) is not carried over with the v2 Drop. | core/deploy `deploy_overlapping_mods_then_purge_restores_game_byte_for_byte`, `purge_restores_original_when_hardlinked_mod_file_is_byte_identical`, `purge_keeps_files_the_game_changed`, `redeploy_purges_previous_deployment_first`; cli/deploy `deploy_verify_status_purge_roundtrip_restores_game`; T17 E2E |
| `verify` | Check deployed files intact | Port | M2 | | core/deploy `verify_reports_modified_and_missing_deployed_files`, `verify_detects_broken_symlink`; cli/deploy `verify_fails_on_changed_or_missing_files`; T17 E2E |
| `status` / `st` | Account, games, deployment state | Port | M2 | | core/deploy `status_reports_deployment_state`; cli/deploy `deploy_verify_status_purge_roundtrip_restores_game`; T17 E2E |
| `launch` / `play` / `run` | Start via Steam, offers to deploy first | Port | M4 | | cli/mods `launch_aliases_need_appid_and_start_steam` |
| `clean` | Delete unused downloads and temp files | Port | M3 | | cli/mods `clean_deletes_only_unreferenced_downloads_after_confirmation` |
| `log [-f] [lines]` | Show log | Port | M1 | `tracing` to file; URL queries and keys are not logged. | cli/cli `log_shows_last_lines_and_commands_are_logged`, `log_follow_streams_new_lines`; modman-cli `tests::logged_command_line_drops_url_queries` |
| Interrupted-deploy recovery | partial | New | M2 | SQLite journal; rollback or resume on next open. | core/deploy `deploy_crash_after_k_files_rolls_back_on_next_open`, `deploy_and_purge_recover_automatically_after_a_crash`, `purge_crash_resumes_on_next_open`, `crash_rollback_restores_original_when_mod_file_is_byte_identical` |
| Refuse deploy/purge while the game runs | no (legacy never blocks; only autoupdate checks, legacy_spec §9.4) | Change | M2 | Difference: deploy and purge are refused while the game process runs; `--dry-run` is still allowed. | core/deploy `deploy_and_purge_refused_while_game_running`, `proc_probe_detects_appid_and_game_folder`, `dry_run_is_allowed_while_game_runs_and_lock_is_held`; cli/deploy `deploy_and_purge_refused_while_game_process_runs` |

## 6. Bethesda specifics (Skyrim SE target)

| Feature | Legacy | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `plugins.txt` writing (`star:` format for SSE/FO4/Starfield, `plain:` for older) | yes | Port | M6 | Location in the Proton prefix `AppData/Local/<folder>`. Purge restores the original `plugins.txt` byte for byte when only modman's entries changed, deletes a file modman created, and otherwise removes only modman's lines (T17F `d7d14fd`; legacy always rewrote the header). The prefix is byte-identical after purge (T17 E2E). | core/loadorder `purge_restores_original_plugins_txt_byte_identical`, `purge_deletes_plugins_txt_modman_created`, `purge_keeps_user_entries_in_modman_created_plugins_txt`, `deploy_writes_star_plugins_txt_crlf_masters_first_and_backs_up_original`, `plain_format_writes_bare_names`, `purge_removes_only_managed_entries`, `stale_managed_entries_are_dropped_on_next_deploy`, `symlinked_plugins_txt_is_refused_and_target_untouched`; cli/plugins `deploy_list_purge_manage_plugins_txt`; T17 E2E |
| Load-order from collection rules | yes | Port | M5 | | core/loadorder `collection_after_rule_is_respected`, `collection_rule_applies_when_loot_cannot_read_plugins`; core::loadorder::rules `after_rule_moves_plugin_behind_its_dependency_case_insensitively`, `cycle_keeps_every_plugin_in_input_order` |
| LOOT plugin sorting | no | New | M6 | libloot 0.29.7 (pure Rust); masterlist downloaded and cached. | core/loadorder `loot_sorts_masters_before_dependents_on_deploy`, `sort_with_loot_orders_fixture_masters_first`, `masterlist_fetch_is_cached_and_used_by_deploy`, `failed_masterlist_fetch_keeps_cache` |
| `plugins` command (list, sort, masterlist) | no | New | M6 | | cli/plugins `deploy_list_purge_manage_plugins_txt`, `plugins_command_rejects_non_bethesda_game`, `plugins_masterlist_downloads_from_override_url` |
| Script-extender and tool requirements (SKSE etc.) | `deps` external list | Port | M3 | | cli/mods `deps_reports_external_tools_and_ack_unack_persist`, `deps_finds_tool_files_in_game_folder`; core::requirements `tool_patterns_match_legacy_regexes`, `tool_file_in_game_folder_counts_unless_deployed_by_modman` |

## 7. Auto-update

| Feature | Legacy | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `autoupdate enable / disable / status / run [--force]` | yes | Port | M7 | Single implementation in core (tech stack §9.2). Refused inside the Flatpak for enable/status/run; `disable` and the GUI startup check are not Flatpak-guarded (accepted, T15F). | cli/autoupdate `enable_status_disable_manage_user_timer_units`, `forced_run_redeploys_and_logs_the_run`, `run_without_games_is_quiet`, `flatpak_sandbox_refuses_enable_status_and_run`; core::autoupdate `first_run_records_baseline_without_acting`, `never_redeploys_while_the_game_runs`, `dry_run_changes_no_state`, `back_off_three_hours_after_failure`; desktop::startup `startup_runs_the_autoupdate_check_once` |
| Schedulers: systemd, cron, autostart | yes | Change | M7 | Difference: systemd user timer only; cron and autostart dropped; `--scheduler` flag removed. | cli/autoupdate `no_scheduler_flag`; core::autoupdate `no_other_scheduler_code`, `enable_writes_units_under_xdg_config_home_with_interval`, `enable_without_user_manager_writes_nothing` |
| Detect running package managers via `/proc` | yes | Port | M7 | | core::autoupdate `package_manager_detected_from_fake_proc`, `package_manager_busy_skips_the_whole_run`, `pacman_db_lock_means_busy` |
| Detect Steam game updates | yes | Port | M7 | `appmanifest` build id. | core::autoupdate `steam_buildid_change_detected_from_fixture_acf`, `system_fingerprint_follows_os_release_and_package_db` |
| Collections auto-update setting, interval setting | yes | Port | M7 | | core::autoupdate `collections_skipped_when_setting_disabled`, `interval_adds_scheduled_check` |

## 8. Web UI (`modman ui`) vs Tauri GUI

| Legacy web view | Status | MS | Notes | Proof |
| :--- | :--- | :--- | :--- | :--- |
| Overview | Port | M4 | | vitest `views.test.tsx`, `App.test.tsx`; T16 flows "K overview: *" (5) |
| Mods | Port | M4 | Conflict view included (M6). | vitest `views.test.tsx`; T16 flows "K mods: *", "G mods: *" |
| Install | Port | M4 | FOMOD wizard (M5). | vitest `views.test.tsx`, `PromptDialog.test.tsx`; T16 flows "K install: *", "G install: *" |
| Collections | Port | M5 | | vitest `views.test.tsx`; T16 flow "K collections: navigate content and back" |
| Requirements | Port | M4 | | vitest `views.test.tsx`; T16 flow "K requirements: navigate content and back" |
| Activity (jobs, log) | Port | M4 | Job progress via core event channel. | vitest `store.test.ts`; desktop::state `jobs_announce_start_and_failure_with_the_message`; T16 flows "K activity", "K logs", "G logs" |
| Settings | Port | M4 | Includes auto-update toggle (M7). | vitest `Settings.test.tsx`; T16 flows "K settings*", "G settings*" |
| GUI technology | Change | M4 | Difference: GUI-only Tauri desktop app replaces the browser web UI (typed `tauri::command`s, strict CSP, least-privilege capabilities). | desktop::tests `csp_has_no_unsafe_eval_and_no_remote_origins`, `capabilities_grant_exactly_the_app_commands_and_event_listening`, `every_listed_command_is_registered_and_defined`, `ts_bindings_list_the_same_commands_and_events`; T17 AppImage GUI smoke |
| `ui [--port N] [--no-open]` browser server (`server.py`) | Drop | - | GUI-only; no headless web mode. | desktop::tests `no_deep_link_plugin_and_no_http_server`; grep: no `server.py`, `--port`, `--no-open`, HTTP server crate (T17 report §2); `modman ui` -> unknown command |
| `ui desktop` (menu entry / Deck non-Steam shortcut) | Change | M7 | Difference: done by the AppImage/deb/rpm/Flatpak `.desktop` files; Deck shortcut documented in `packaging/README.md`. | T17 bundle check: deb and rpm ship `modman.desktop` and `modman-nxm.desktop`; `packaging/common/org.modman.app.desktop` |
| `ui-state`, `ui-detect`, `ui-thumbs` (JSON helpers for the UI) | Drop | - | Replaced by typed `tauri::command`s; thumbnails cached in core. | desktop::tests `no_deep_link_plugin_and_no_http_server` (asserts the names are absent); `modman ui-state` -> unknown command; core::app::thumbs `cache_paths_reject_traversal_and_honour_the_ttl` |
| Token + localhost-only server security | Drop | - | No server. | grep: no token/server code (T17 report §2); desktop::tests `no_deep_link_plugin_and_no_http_server` |
| Controller / spatial navigation | New | M4 | See tech stack §4.4. | vitest `spatial.test.ts`, `gamepad.test.ts`, `actions.test.ts`, `Dialog.test.tsx`; T16 flows "G *" (10), audit 0 unreachable / 0 traps, axe 0 issues |

## 9. Needs a decision or a code read before porting

Resolved by T01 (`legacy_spec.md` §0):

1. `game set` keys: nine keys (§2 above).
2. FOMOD modes and choice storage: legacy_spec §8 (§4 above).
3. `deps` data: Nexus requirements + external tool regexes (§3 above).
4. Legacy never blocks deploy while the game runs; the rewrite does (§5 above, Change).
5. Aliases and flags: legacy_spec §1; diffed by T17 (the `deploy -n` gap was closed in T17F `d7d14fd`).
6. Unlisted behaviours (rate limit, adult, notifications, lock): legacy_spec §16; rate limit proven by core/nexus `http_429_backs_off_then_succeeds`, `exhausted_rate_limit_throttles_and_persists_across_clients`.

End-to-end acceptance test for v1 (fresh install -> `login` -> `game detect` -> install a Skyrim SE mod -> `deploy` -> `verify` -> `purge` -> game folder byte-identical): executed by T17 with a mock Nexus and a fixture Steam root and a local archive; see `docs/plan/20261020-rust-migration/t17_report.md` §4.
