# D1: why `cargo test --workspace` stalls

Date: 2026-10-31. Diagnosis only. No source files were changed.

## Verdict

There are two separate causes. Neither one is in the test code.

1. **Agent "waiting on background work" that never ends.** Earlier agents waited on the build with `until ! pgrep -f "cargo test --workspace" ...; do sleep N; done`. That loop runs inside `bash -c '<script>'`, so the shell's own command line contains the pattern. `pgrep -f` matches the waiting shell itself, and the loop never exits. Three of these pollers are still running:
   - PID 324026: about 18 h old, waits on `cargo test --workspace --no-run`
   - PID 624784: about 1.5 h old, waits on T2-validation (the run behind it finished at 13:29)
   - PID 675144: about 30 min old, waits on B1-validation

   Check: `pgrep -af "cargo test --workspace"` with no cargo running still lists these shells and the shell running the check. The same mistake ran during this diagnosis: `pkill -f RUSTC_BOOTSTRAP` killed its own shell (exit 144).
2. **Builds that really are slow (about 13-14 min), almost all of it link time on btrfs.** When `--workspace` links its test binaries, about 16 `rust-lld` processes run at once. Each runs 16 threads and writes a 240-480 MB debug binary. The target directory is on btrfs with `compress=zstd:1`, and the disk is 87% full. Every linker thread sits in uninterruptible sleep (state D) at kernel wait point `handle_reserve_ticket`, which is btrfs waiting for space reservation. Meanwhile one kworker (`events_unbound`) uses about 90% CPU on reclaim. The `.tmp` output files did not grow at all during a 20 s sample. Load average reached about 245.

## Measurements

| Phase | Result |
|---|---|
| Build 1, `cargo test --workspace --no-run --timings` (workspace crates only) | **805 s** (13m25s) |
| Build 2, same command (159 crates recompiled after a fingerprint change I caused by mistake) | **826 s** (13m46s). Dependencies: 261 s of CPU in total, done within the first ~15 s of wall time |
| Per-unit time from cargo timings: vessel_core lib(test) / vessel bin / nexus_secrets (1 test) / cli / restage | 696 / 620 / 558 / 530 / 530 s. A 1-test binary taking 558 s can only be link time |
| One test binary relinked alone (`ue4ss_mods_txt`, 242 MB) | **0.5 s** |
| Sampled during build 2 | rustc=16, rust-lld=16, 176-249 rust-lld threads in D state (`handle_reserve_ticket`), btrfs metadata may_use about 1.5-1.65 GiB plus used 2.6 GiB, against 4 GiB of metadata chunks |
| All 22 test binaries run, `timeout 300` each, `--report-time` | **about 5.3 s in total**, all passing. vessel-core lib: 482 passed, 2 ignored, 1.70 s |
| Slowest single tests | `migrate::tests::crash_after_any_step_converges_on_rerun` 1.01 s; `loaders::runner::tests::normal_exit_kills_leftover_group_members` 0.85 s (`crates/vessel-core/src/loaders/runner.rs:1400`, a deliberate 800 ms sleep); `commands::loader_tests::log_tail_streams_whole_redacted_lines` 0.60 s; `timeout_kills_the_whole_group` 0.51 s |

No test hangs. There are no sleeps or timeouts that matter, and nothing waits on the network, stdin or a lock. Tests use wiremock or localhost, and the retry steps are cut to 1-5 ms in tests (`tests/nexus.rs:27`, `tests/collections.rs:23`).

Lock contention: no build-directory lock contention (no "Blocking waiting for file lock") and no rust-analyzer process. A `target/debug/vessel-desktop` (PID 696297, started before this task) is running. It belongs to the user or another agent and was left alone. It does not block linking.

The environment: btrfs on `/home` (`compress=zstd:1,ssd,discard=async`) with 19.5 GiB unallocated, 87% used. `target/` takes 71 GB (deps 45 GB, incremental 21 GB) and holds 47 binaries over 50 MB from the last 2 days (12.1 GB). No `[profile]` overrides, so debug builds get full debuginfo. The linker is rust-lld, the default, called through `cc`. The machine has 16 cores and 30 GB of RAM.

## Fixes, ranked by impact

1. **Stop the stuck pollers and change how agents wait.** Kill PIDs 324026, 624784 and 675144; I left them running because they are not mine. Agents should never use `pgrep -f "<cmdline fragment>"` or `pkill -f` from a `bash -c` wrapper. Wait on a PID instead (`while kill -0 $PID; do ...`), use `pgrep -x cargo`, or rely on run_in_background completion notifications. Always wrap waits in `timeout`.
2. **Make link output much smaller** by adding this to the root `Cargo.toml`:
   ```toml
   [profile.dev]
   debug = "line-tables-only"
   split-debuginfo = "unpacked"
   [profile.dev.package."*"]
   debug = false
   ```
   This cuts each binary from 240-480 MB to a small fraction of that, and with it the btrfs reservation pressure that drives the stall. It also affects `[profile.test]`, which inherits from dev.
3. **Take the target directory off compressed copy-on-write btrfs.** Recreate it with `chattr +C`, or run `btrfs property set target compression none`. Alternatively, set `CARGO_TARGET_DIR` to a non-btrfs or NOCOW location. tmpfs (15 GB) is too small for the current 71 GB target.
4. **Free space and metadata headroom.** Run `cargo clean`, which takes 71 GB down to one fresh build. As root, `btrfs balance start -dusage=50 /home` gives unallocated space back to metadata.
5. **Limit link concurrency** while the cause stays: `CARGO_BUILD_JOBS=4` or `cargo test -j4`, or add `-C link-arg=-Wl,--threads=4` in `.cargo/config.toml`. This is lower impact and slows compilation.
6. **Use the same cargo invocation every time.** `cargo test -p vessel-core` and `cargo test --workspace` unify features differently, so they produce different artifact hashes: `ue4ss_mods_txt-494a…` vs `-9306…`. Alternating between them doubles the link work and the size of `target/`. Do not export `RUSTC_BOOTSTRAP` to cargo, because it invalidates every fingerprint and forces a full rebuild.
7. Optional: `cargo-nextest` with `slow-timeout = { period = "30s", terminate-after = 4 }` as a guard. The tests do not need it today.
