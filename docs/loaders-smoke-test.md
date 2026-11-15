# SMAPI smoke test (manual)

**You run this test yourself. No agent or CI job runs it.** It runs the real SMAPI installer against your real Stardew Valley install. It is opt-in, and needs a Steam account that owns the game.

For what each step does, see [loaders.md](loaders.md). Write down the result of every "Expected" line. Any mismatch is a failure; keep the run's log from `~/.local/share/vessel/loaders/runs/`.

## Before you start

- **Back up your saves.** SMAPI's installer changes files outside the game folder, and Vessel cannot undo those changes.
  - Native saves: `~/.config/StardewValley/`
  - Proton saves: `<compatdata>/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/`
- **Use a non-Flatpak Steam.** Use a Vessel build that is not the Flatpak for the Proton part.
- **Start clean.** Stardew Valley must have no SMAPI installed and no Steam launch options. If it has either, uninstall SMAPI, clear the options, and verify the game files in Steam first.
- **Set shell variables.** The commands below use:

  ```sh
  G="$HOME/.local/share/Steam/steamapps/common/Stardew Valley"   # your game folder
  OUT="$HOME/smapi-smoke"; mkdir -p "$OUT"
  ```

## A. Native

### A1. Baseline

1. In Steam, open the game's Properties > Compatibility and make sure no compatibility tool is forced. Let Steam finish any download.
2. Start the game once from Steam, then quit.
3. Record the baseline:

   ```sh
   (cd "$G" && ls -la > "$OUT/native-before.txt" && sha256sum StardewValley "Stardew Valley" steam_appid.txt > "$OUT/native-before.sha")
   head -c 4 "$G/Stardew Valley" | od -c | head -1   # expect 177 E L F
   grep -c StardewModdingAPI "$G/StardewValley"       # expect 0
   ls -la ~/.config/StardewValley > "$OUT/native-appdata-before.txt"
   ```

**Expected:** `StardewValley` is a bash script, `Stardew Valley` is an ELF, and there is no `StardewModdingAPI.dll` and no `StardewValley-original`.

### A2. Install

1. Download SMAPI through Vessel, from Nexus (Stardew Valley mod 2400).

   **Expected:**
   - Vessel says the download is a mod loader to review in Requirements.
   - No mod row is added.
   - Nothing appears under `"$G/Mods"`.
2. Open the SMAPI card in Requirements and start the pending install.

   **Expected**, in the consent dialog:
   - Operation install, the SMAPI version you downloaded, runtime native.
   - Provenance verified.
   - The note that the installer runs with your user permissions and is not sandboxed.
   - One command: `…/loaders/scratch/<id>/SMAPI <ver> installer/internal/linux/SMAPI.Installer --install --no-prompt --game-path <G>`.
   - Environment: only `PATH`, `HOME`, `LANG=C.UTF-8` and `TMPDIR`.
   - The touch scope, listing `StardewValley`, `StardewModdingAPI*`, `smapi-internal/**` and `steam_appid.txt`.
   - Three unrecoverable changes: the `~/.config/StardewValley/Saves/.smapi` move, the `ErrorLogs` delete, and the about-40 MB temp copy.
3. Confirm.

   **Expected:**
   - The run succeeds.
   - The SMAPI card shows installed with the version.
   - No launch options are requested.
4. Check the game folder:

   ```sh
   grep -c StardewModdingAPI "$G/StardewValley"                  # expect >= 1
   (cd "$G" && sha256sum StardewValley-original "Stardew Valley")  # compare with native-before.sha
   ls -l "$G/StardewValley" "$G/StardewModdingAPI"               # expect -rwxr-xr-x
   ls -d "$G/StardewModdingAPI.dll" "$G/smapi-internal" "$G/Mods/ConsoleCommands" "$G/Mods/SaveBackup"
   ```

   **Expected:**
   - `StardewValley-original` has the baseline `StardewValley` hash.
   - `Stardew Valley` is unchanged.
   - All listed paths exist.
   - `StardewValley` and `StardewModdingAPI` have mode 755.

### A3. Launch

1. Start the game from Steam.

   **Expected:** SMAPI starts and the game loads with SMAPI's console mods. `~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt` is written.
2. Quit the game.

### A4. Steam verify

1. In Steam, run Properties > Installed Files > Verify integrity of game files.
2. Run:

   ```sh
   grep -c StardewModdingAPI "$G/StardewValley"   # expect 0
   ```

3. Reopen or refresh the SMAPI card.

**Expected:** `StardewValley` is the vanilla script again. The SMAPI card shows SMAPI as changed since install, with the reason "StardewValley is no longer SMAPI's launcher".

### A5. Repair

1. Start repair from the SMAPI card.

   **Expected:** the consent dialog shows operation repair, runtime native, and the same installer command with `--install`. You do not have to download anything.
2. Confirm.

   **Expected:**
   - The run succeeds, with the warning "StardewValley is the vanilla file again."
   - `grep -c StardewModdingAPI "$G/StardewValley"` is at least 1.
   - `StardewValley-original` still has the baseline hash.
   - The card shows installed.
3. Start the game from Steam.

   **Expected:** SMAPI starts. Quit.

### A6. Uninstall and confirm vanilla

1. Start uninstall from the SMAPI card.

   **Expected:** the consent dialog shows the installer with `--uninstall`.
2. Confirm.

   **Expected:** the run succeeds and the card shows not installed.
3. Check the game folder:

   ```sh
   (cd "$G" && ls -la > "$OUT/native-after.txt" && sha256sum StardewValley "Stardew Valley" steam_appid.txt | diff - "$OUT/native-before.sha")
   ls "$G" | grep -i -e StardewModdingAPI -e smapi-internal -e StardewValley-original   # expect no output
   ls "$G/Mods"
   ```

   **Expected:**
   - The `sha256sum` diff is empty: `StardewValley`, `Stardew Valley` and `steam_appid.txt` match the baseline.
   - No SMAPI files remain.
   - `Mods/ConsoleCommands` and `Mods/SaveBackup` are gone.
   - Your own mods under `Mods/` are untouched.
4. Run Steam verify again.

   **Expected:** Steam reports all files validated, or re-acquires none of the files you checked.

## B. Proton

### B1. Baseline

1. In Steam, open Properties > Compatibility, force a Proton version, and let Steam download the Windows build.
2. Start the game once from Steam, so that the prefix `steamapps/compatdata/413150/pfx` exists. Quit.
3. Make sure the game's Launch Options field is empty.
4. Record the baseline:

   ```sh
   P="$HOME/.local/share/Steam/steamapps/compatdata/413150/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley"
   (cd "$G" && ls -la > "$OUT/proton-before.txt" && find . -maxdepth 1 -type f -exec sha256sum {} + | sort -k2 > "$OUT/proton-before.sha")
   ls -la "$P" > "$OUT/proton-appdata-before.txt"
   ```

**Expected:** `Stardew Valley.exe` is present, and there is no `StardewModdingAPI.dll`.

### B2. Install

1. Download SMAPI through Vessel, from Nexus mod 2400.

   **Expected:** a pending preview in Requirements, and no mod row.
2. Start the install from the SMAPI card.

   **Expected**, in the consent dialog:
   - Runtime Proton, with the Proton you forced.
   - The command runs `<proton> run` (wrapped in the Steam Linux Runtime `run` if your Proton needs one) on `…/internal/windows/SMAPI.Installer.exe --install --no-prompt --game-path Z:\…`.
   - Environment: `PATH`, `HOME`, `LANG`, `TMPDIR`, `STEAM_COMPAT_DATA_PATH` and `STEAM_COMPAT_CLIENT_INSTALL_PATH` only.
   - The unrecoverable changes name the prefix's `AppData/Roaming/StardewValley` and `AppData/Local/Temp/SMAPI-installer-*`.
   - Provenance verified.
3. Confirm.

   **Expected:**
   - The run succeeds.
   - `StardewModdingAPI.dll`, `StardewModdingAPI.exe` and `smapi-internal/` exist in `"$G"`.
   - The SMAPI card asks for this launch option:

     ```text
     "Z:\<game folder with \ separators>\StardewModdingAPI.exe" %command%
     ```

### B3. Launch options

Test both paths.

1. **Copy and acknowledge.**
   1. Copy the string from the card and paste it into Steam's Launch Options.
   2. Acknowledge it in Vessel.

   **Expected:** the card stops asking for launch options.
2. **Apply.**
   1. Clear the field in Steam.
   2. Quit Steam completely.
   3. Use Apply in Vessel.

   **Expected:**
   - With Steam still running, Apply is refused (`launch_steam_running`).
   - With several Steam accounts, Vessel asks you to pick one, and the most recently used one is preselected.
   - After Apply, `userdata/<account>/config/localconfig.vdf.vessel-bak` exists.
   - When you start Steam, the game's Launch Options field shows the string.

### B4. Launch

1. Start the game from Steam.

   **Expected:** SMAPI's console window opens and the game loads. `"$P/ErrorLogs/SMAPI-latest.txt"` is written.
2. Quit the game.

### B5. Steam verify

1. Run Verify integrity of game files.

   **Expected:** the SMAPI card shows installed. Under Proton, Vessel reports drift only when `StardewModdingAPI.dll` is missing. The launch options are unchanged.

### B6. Repair

1. Start repair from the SMAPI card, and confirm.

   **Expected:**
   - The consent dialog shows operation repair with the Windows installer.
   - The run succeeds.
   - The SMAPI files are present.
   - The launch option asked for is unchanged.
2. Start the game.

   **Expected:** SMAPI starts. Quit.

### B7. Uninstall and confirm vanilla

1. Start uninstall from the SMAPI card, and confirm.

   **Expected:**
   - The run succeeds.
   - The card shows not installed.
   - The card no longer asks for launch options.
2. Remove the launch option: quit Steam and use Apply (Vessel clears the value it wrote), or clear the field by hand.
3. Check the game folder:

   ```sh
   (cd "$G" && find . -maxdepth 1 -type f -exec sha256sum {} + | sort -k2 | diff - "$OUT/proton-before.sha")
   ls "$G" | grep -i -e StardewModdingAPI -e smapi-internal   # expect no output
   ```

   **Expected:** the diff is empty, and no SMAPI files remain.
4. Start the game from Steam.

   **Expected:** the vanilla game starts, with no SMAPI console. Quit.
5. Run Steam verify.

   **Expected:** no files are re-acquired.

## C. Adopt a manual install

Run this on the native setup, starting from the vanilla state of A6.

1. Install SMAPI by hand, outside Vessel. Extract the SMAPI zip to a temporary folder, then run:

   ```sh
   "/tmp/smapi-manual/SMAPI <ver> installer/internal/linux/SMAPI.Installer" --install --no-prompt --game-path "$G"
   ```

2. Open the SMAPI card in Requirements.

   **Expected:** SMAPI shows as installed outside Vessel.
3. Download SMAPI through Vessel, then open the pending preview.

   **Expected:** the operation is adopt, and the dialog lists two installer runs: `--uninstall`, then `--install`.
4. Confirm.

   **Expected:**
   - The run succeeds.
   - The card shows installed, managed by Vessel.
   - `StardewValley` is SMAPI's launcher.
   - `StardewValley-original` has the baseline vanilla hash.
5. Uninstall from the SMAPI card, then repeat the checks of A6 step 3.

   **Expected:** the folder matches the baseline, and no SMAPI files remain.

## D. Rollback

Run this on the native setup, starting from vanilla. It needs two terminals.

### D1. Failed run rolls back

1. In the second terminal, start a loop that freezes the installer as soon as it appears:

   ```sh
   until pkill -STOP -f SMAPI.Installer; do sleep 0.05; done
   ```

   Then start an install from the SMAPI card and confirm. The loop exits once the installer is frozen.

2. While the installer is frozen, change two in-scope paths:

   ```sh
   printf '\n# smoke\n' >> "$G/StardewValley"; touch "$G/StardewModdingAPI.smoke"
   ```

3. Wait for the 5-minute timeout.

   **Expected:**
   - The run fails as timed out.
   - The game folder is back to its state before the run: `StardewModdingAPI.smoke` is gone, and the `sha256sum` check of A6 step 3 matches the baseline.
   - The card shows not installed.
   - The run's log under `~/.local/share/vessel/loaders/runs/` is kept.

### D2. Crash recovery offers rollback

1. Start the freeze loop, then an install, and confirm, as in D1 step 1.
2. Run step 2 of D1 again.
3. Kill Vessel:

   ```sh
   pkill -KILL -x vessel-desktop
   ```

   **Expected:** the installer process is gone too (`pgrep -f SMAPI.Installer` prints nothing).
4. Start Vessel again.

   **Expected:** Vessel reports the interrupted SMAPI run and offers a rollback.
5. Accept the rollback.

   **Expected:**
   - The game folder matches the baseline.
   - `StardewModdingAPI.smoke` is gone.
   - The card shows not installed.

   If Vessel offers no rollback, record that as a failure.

## E. Clean up

- Delete `"$OUT"` and the temporary SMAPI folder.
- To remove the Proton test state, clear the compatibility tool in Steam and verify the game files.
- Your saves were backed up before you started. Compare them with `~/.config/StardewValley/Saves` (native) and `"$P/Saves"` (Proton).
