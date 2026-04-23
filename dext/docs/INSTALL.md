# Installing Maschine (macOS)

This guide covers two install flows:

- **[Developer mode](#1-developer-mode-no-apple-developer-program-required)** — run a local build on your own Mac. SIP must be off and system-extension developer mode on. Works without an Apple Developer Program membership.
- **[Production install](#2-production-install-requires-entitlement-from-apple)** — install a signed + notarized `.pkg` downloaded from a release. Works on a default-locked Mac.

Both flows end with the same thing: the Maschine dext loaded, `maschined` running, and a Mk3 that lights up when plugged in.

Jump to [uninstall](#3-uninstall) or [troubleshooting](#4-troubleshooting) if you're already installed.

---

## 1. Developer mode (no Apple Developer Program required)

This is the path the project uses today. Until Apple approves the distribution entitlements for `com.cantonic.maschine.dext`, the **production** flow below will not work for anyone else — but the dev flow works on any Mac you administer.

### 1.1 One-time host setup

Both steps survive reboots; you only do them once.

**Step A — disable System Integrity Protection.** macOS refuses to load a `.development`-signed dext while SIP is on. This is unavoidable; it's how Apple sandboxes unshipped drivers.

1. Shut the Mac down.
2. Apple Silicon: hold the power button until "Options" appears → **Options → Continue**.
   Intel: hold ⌘-R while booting.
3. In the recovery menu: **Utilities → Terminal**, then run:
   ```bash
   csrutil disable
   ```
4. Apple Silicon only — when prompted, lower security policy to **Reduced Security** and allow user-management of kernel extensions. Confirm with your admin password.
5. Reboot normally.

Verify back in macOS:
```bash
csrutil status
# expected: "System Integrity Protection status: disabled."
```

**Step B — turn on system-extension developer mode.**
```bash
sudo systemextensionsctl developer on
```
This lets you re-install the same dext bundle ID repeatedly without bumping the version number, which you'll do a lot during development.

### 1.2 Build and install

From the repo root:
```bash
./dext/scripts/build-dev.sh
```

If Xcode is installed at `/Applications/Xcode.app`, this produces `dext/build/Maschine.app`. It's either unsigned (if you have no codesigning identity) or ad-hoc signed with whatever identity `security find-identity` returns first — both are fine in dev mode.

Launch it:
```bash
open dext/build/Maschine.app
```
or from the CLI:
```bash
dext/build/Maschine.app/Contents/MacOS/MaschineHost
```

You'll see:
```
[MaschineHost] submitted activation request for com.cantonic.maschine.dext
[MaschineHost] user approval needed — open System Settings → General → Login Items & Extensions
```

### 1.3 Approve the dext in System Settings

On macOS 15 Sequoia (and 16+):

1. **System Settings → General → Login Items & Extensions**
2. Scroll to **Extensions** near the bottom.
3. Find **Driver Extensions**, click the **(i)** button next to it.
4. Toggle **Maschine** on.
5. Authenticate with Touch ID or admin password.
6. macOS will prompt for a **reboot** on first activation of a new dext bundle ID. Reboot.

After the reboot, plug in your Mk3:
```bash
systemextensionsctl list
# expected line: [activated enabled] com.cantonic.maschine.dext
log stream --predicate 'sender == "MaschineMk3Dext"'
# expected: "MaschineMk3HidTransport::Start succeeded"
```

> **macOS 13/14 note.** On Ventura and Sonoma the path is **System Settings → Privacy & Security → scroll to "Security"** and you'll see a yellow banner: *"System software from Can Tonic was blocked from loading"* with an **Allow** button. Same end result, different UI.

---

## 2. Production install (requires entitlement from Apple)

This flow uses a signed + notarized `.pkg`. It does **not** require SIP off, does **not** require developer mode, and works for any user with admin rights.

**Status as of 0.1.0:** this path is gated on Apple approving `com.apple.developer.driverkit`, `driverkit.transport.usb` (for VID `0x17CC`), `driverkit.family.usb.pipe`, and `driverkit.userclient-access` for our Team ID. See `docs/R2-packaging.md` §2. Until that lands, the `.pkg` produced by `dext/scripts/build-dist.sh` will sign and notarize but the dext it contains **will refuse to activate** on anyone else's Mac.

### 2.1 Download and install

1. Download **Maschine-Mk3-Host-0.1.0.pkg** from the release.
2. Double-click the `.pkg`. Gatekeeper will verify the notarization ticket offline (no network needed). The installer walks you through license → destination → admin authentication.
3. When the installer finishes, `/Applications/Maschine.app` exists.

### 2.2 Choose a launch model

The `.pkg` comes in two variants; the release notes will say which one you downloaded:

**app-launched (default).** `maschined` only runs while you keep `Maschine.app` open. Good for casual use.
- To start: **Applications → Maschine.app** (double-click).
- To stop: quit Maschine.app from the menu bar / Dock.

**LaunchDaemon (opt-in).** `maschined` runs from system boot regardless of who's logged in, and relaunches on crash. Good if you want your controller to work in every DAW without ever thinking about it.
- The `.pkg` postinstall hook already wrote `/Library/LaunchDaemons/com.cantonic.maschined.plist` and loaded it. `maschined` is running now.
- Log output lands at `/var/log/maschined.log` and `/var/log/maschined.err.log`.

You can tell which variant was installed:
```bash
sudo launchctl print system/com.cantonic.maschined 2>&1 | head -1
# "system/com.cantonic.maschined = <...>" → LaunchDaemon variant
# "Could not find service" → app-launched variant
```

### 2.3 First-run approval (same as dev mode)

The first launch of `Maschine.app` (app-launched) or first boot after install (LaunchDaemon) triggers the same System Settings prompt as step 1.3 above. Click **Open System Settings**, go to **General → Login Items & Extensions → Driver Extensions → (i)**, toggle Maschine on. macOS will ask for a reboot; reboot. Done.

Subsequent updates (same bundle ID, higher version) do **not** need re-approval and do **not** need a reboot.

---

## 3. Uninstall

Same command for both install flows:
```bash
sudo /Applications/Maschine.app/Contents/Resources/uninstall.sh
```
If the shipped `.pkg` didn't drop `uninstall.sh` inside the app, grab it from the repo:
```bash
sudo dext/scripts/uninstall.sh
```

What it does, in order:
1. Unloads and removes `/Library/LaunchDaemons/com.cantonic.maschined.plist` (if present).
2. `systemextensionsctl uninstall $TEAM_ID com.cantonic.maschine.dext`.
3. `rm -rf /Applications/Maschine.app`.
4. Removes Maschine preferences and caches.

If `systemextensionsctl` refuses the uninstall (common on a fresh macOS install that's never seen the dext), finish it by hand in **System Settings → General → Login Items & Extensions → Driver Extensions → (i)**, toggle Maschine off, then re-run the script.

**Panic button:** if everything is wedged, `sudo systemextensionsctl reset` removes every user-installed system extension on the machine. Use only as a last resort — this kills every third-party dext, not just ours.

---

## 4. Troubleshooting

### 4.1 "Maschine was blocked from use because it is not from an identified developer"

You're running a dev build (`build-dev.sh`) on a Mac where SIP is still on. Dev builds are not codesigned with a Developer ID — Gatekeeper will reject them. Two options:
1. **Disable SIP** (follow §1.1 Step A). This is required for the dev flow anyway.
2. **Use the production .pkg** instead — but that only exists once Apple approves our entitlements.

### 4.2 "System extension cannot be used" / dext stuck in `[activated waiting for user]`

The approval step in System Settings didn't complete. Go to **System Settings → General → Login Items & Extensions → Driver Extensions → (i)**, toggle Maschine on, authenticate, reboot. Dev vs distribution build doesn't matter for this step — same UI either way.

### 4.3 Nothing at all shown under "Driver Extensions"

macOS never saw the activation request. Likely causes:
- `Maschine.app` crashed before `OSSystemExtensionRequest.submitRequest` ran — check `console.app` for crashes signed by your identity.
- You're running under the LaunchDaemon variant but `maschined` itself doesn't actually submit the activation request — the `.app` must have been launched at least once to register the dext. Run `open /Applications/Maschine.app` by hand once.
- The `.pkg` didn't land the dext under `Maschine.app/Contents/Library/SystemExtensions/`. Check with `ls /Applications/Maschine.app/Contents/Library/SystemExtensions/`.

### 4.4 `codesign --verify` complains about the dext after you codesigned the `.app`

You used `codesign --deep`. Don't. It strips the dext's entitlements plist and the notary service will reject the submission. Sign bottom-up in this explicit order:
1. `maschined` (if bundled) and the host Mach-O.
2. The dext's inner Mach-O.
3. The `.dext` bundle with `--entitlements MaschineMk3Dext.entitlements`.
4. The `.app` bundle with `--entitlements MaschineHost.entitlements`.

`dext/scripts/build-dist.sh` does this correctly — compare your commands to its output if you're doing it by hand.

### 4.5 Dev build works on your Mac, breaks for everyone else

The dev build uses the `.development` form of the DriverKit entitlements. Those only work on Macs with developer mode turned on and SIP off. This is Apple's sandbox: a `.development`-signed dext is rejected on a locked Mac no matter who signed it.

You need the **distribution** entitlements for anyone else to run it, and those require filing at <https://developer.apple.com/contact/request/system-extension/> with your Team ID and a justification for matching NI's USB VID `0x17CC`. See `docs/R2-packaging.md` §2. Budget 2–8 weeks.

### 4.6 `notarytool` rejects the submission

Fetch the log:
```bash
xcrun notarytool log <submission-id> \
  --apple-id "$MASCHINE_APPLE_ID" \
  --team-id "$MASCHINE_TEAM_ID" \
  --password "$MASCHINE_NOTARY_PASSWORD"
```
Common causes in this project:
- Hardened Runtime missing on a nested Mach-O. `build-dist.sh` passes `--options runtime` on every `codesign` call — if you edited that, restore it.
- `--deep` used on an outer bundle. See 4.4.
- Bundle ID in the entitlements doesn't match the actual bundle ID. We expect `com.cantonic.maschine` and `com.cantonic.maschine.dext` — don't edit one without the other.

### 4.7 Mk3 plugged in but nothing shows up in logs

The dext loaded but didn't match the interfaces. Verify the device is at VID `0x17CC` / PID `0x1600`:
```bash
system_profiler SPUSBDataType | grep -A 5 'Maschine'
```
If the VID/PID differ (e.g. Mk1, Mk2, or a firmware revision that shifted interfaces), `MaschineMk3Dext/Info.plist` won't match and nothing attaches. File an issue with the `system_profiler` output.

---

## 5. What's installed where

| Path | Who owns it | Removed by |
|---|---|---|
| `/Applications/Maschine.app` | the `.pkg` / `build-dev.sh` copy | `uninstall.sh` |
| `/Applications/Maschine.app/Contents/Library/SystemExtensions/MaschineMk3Dext.dext` | the app bundle | removed with the `.app` |
| `/Library/LaunchDaemons/com.cantonic.maschined.plist` | `install-daemon.sh` (LaunchDaemon variant only) | `uninstall.sh` |
| `/var/log/maschined.{log,err.log}` | LaunchDaemon writes these | left behind intentionally (diagnostics) |
| `~/Library/Preferences/com.cantonic.maschine.plist` | the app on first run | `uninstall.sh` |
| `~/Library/Caches/com.cantonic.maschine/` | the app | `uninstall.sh` |

The LaunchDaemon label is `com.cantonic.maschined`. Dext bundle ID is `com.cantonic.maschine.dext`. Host app bundle ID is `com.cantonic.maschine`.
