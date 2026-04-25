# Maschine DriverKit Extension

Xcode project scaffold for the Maschine Mk3 DriverKit system extension
(dext) and the minimal host `.app` that activates it.

- Host app bundle ID: `com.cannuri.maschine`
- Dext bundle ID: `com.cannuri.maschine.dext`
- Personalities: `IOUSBHostInterface` on `idVendor=0x17CC` (6092),
  `idProduct=0x1600` (5632), `bInterfaceNumber` 4 (HID) and 5 (displays)
- Deployment targets: macOS 14.0+, DriverKit 23.0+

This repository currently corresponds to **milestone M1** per
`docs/A1-architecture.md`: the dext loads and logs `Start`/`Stop` on
attach/detach. There is no real USB I/O yet — that lands in `I1`.

---

## 0. Apple Team ID placeholder

Every place a real Apple Developer Team ID is eventually required is
marked `XXXXXXXXXX` (ten capital X's). To lock the project to a real
team, run:

```bash
# From the repo root; pick a real 10-char team ID and paste it in.
TEAM="ABCDE12345"
grep -rl 'XXXXXXXXXX' dext | xargs sed -i '' "s/XXXXXXXXXX/$TEAM/g"
```

Until a Team ID is set, the project signs ad-hoc (`CODE_SIGN_IDENTITY=-`),
which is fine for local `systemextensionsctl developer on` testing but
will not notarise or install on a default-locked Mac.

---

## 1. Prerequisites (one-time)

The bring-up milestone requires **SIP off** and **system-extension
developer mode on**. Both are macOS-global settings that survive reboots.

### 1.1 SIP off

From the login screen hold ⌘R to boot into Recovery. Open
Terminal → run:

```bash
csrutil disable
```

Reboot. On Apple Silicon you will also be prompted to lower security to
"Reduced Security" — say yes.

### 1.2 Developer mode for system extensions

After the reboot, back in macOS:

```bash
sudo systemextensionsctl developer on
```

This flag skips the version-bump check and lets you re-install the same
bundle identifier repeatedly during iteration.

### 1.3 Xcode

`./build.sh` auto-selects `/Applications/Xcode.app` via
`DEVELOPER_DIR`. If you want to pin the active developer directory
globally:

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

Xcode 15 or newer is required (DriverKit 23.0+ SDK). This scaffold is
built and tested against Xcode 16.2 / DriverKit 24.2 SDK.

---

## 2. Build

```bash
./build.sh                          # default: Debug
CONFIG=Release ./build.sh            # release build
```

Artefacts land under `./build/`:

- `./build/Maschine.app`
  — the copy of the host `.app` with the dext embedded under
  `Contents/Library/SystemExtensions/MaschineMk3Dext.dext`
- `./build/Build/Products/<Config>/MaschineHost.app`
  — the Xcode-native derived-data copy (same bits)

The script is idempotent — rerunning it is safe, but a clean wipe is
just `rm -rf build`.

### Warnings / signing

Local builds sign ad-hoc (`CODE_SIGN_IDENTITY=-`) with no provisioning
profile. Xcode will log a "signing with ad-hoc identity" warning for
both targets; that is expected for M1-era development.

---

## 3. Install the dev build

The host `.app` is its own installer. It submits an
`OSSystemExtensionRequest.activationRequest` for the embedded dext on
launch.

```bash
./build/Maschine.app/Contents/MacOS/MaschineHost
```

stdout will look roughly like:

```
[MaschineHost] submitted activation request for com.cannuri.maschine.dext
[MaschineHost] user approval needed — open System Settings → General → Login Items & Extensions
```

Open **System Settings → General → Login Items & Extensions → Driver
Extensions → (i)**, toggle the Maschine entry on, authenticate.
macOS will prompt for a **reboot** on first activation of a given dext
bundle ID. Subsequent reinstalls during iteration do not reboot.

After reboot:

```bash
systemextensionsctl list                              # expect: [activated enabled]
log stream --predicate 'sender == "MaschineMk3Dext"' # watch the dext talk
```

Plugging in the Mk3 at that point should emit:

```
MaschineMk3HidTransport::Start succeeded — Mk3 HID interface (if#4) attached
MaschineMk3DisplayTransport::Start succeeded — Mk3 display interface (if#5) attached
```

---

## 4. Uninstall

```bash
systemextensionsctl uninstall XXXXXXXXXX com.cannuri.maschine.dext
```

Replace `XXXXXXXXXX` with your team ID once set (see §0). In ad-hoc /
dev-mode builds you may also use the literal `-` as the team argument.

Panic button for when the state gets wedged:

```bash
sudo systemextensionsctl reset
```

This uninstalls every user-installed system extension on the machine —
use sparingly.

---

## 5. Project layout

```
dext/
├─ MaschineDext.xcodeproj/         # Xcode 15+ project, two targets
├─ MaschineHost/                   # .app container (Swift, LSBackgroundOnly)
│  ├─ main.swift                   # OSSystemExtensionRequest.activationRequest
│  ├─ Info.plist
│  └─ MaschineHost.entitlements    # system-extension.install + userclient-access
├─ MaschineMk3Dext/                # .dext target (C++/IIG)
│  ├─ MaschineMk3HidTransport.{iig,cpp}     # if#4 personality (HID)
│  ├─ MaschineMk3DisplayTransport.{iig,cpp} # if#5 personality (displays)
│  ├─ MaschineMk3UserClient.{iig,cpp}       # user-client skeleton
│  ├─ Info.plist                             # IOKitPersonalities
│  └─ MaschineMk3Dext.entitlements            # driverkit + transport.usb + family.usb.pipe + userclient-access
├─ build.sh
├─ docs/                           # R1/R2/R3/A1 design docs
└─ README.md                       # you are here
```

---

## 6. Codesigning notes (mirrors R2 §4)

Shipping builds must sign bottom-up — Mach-O → dext → app — with the
`Developer ID Application` identity and the Hardened Runtime enabled
(`--options runtime`). Never use `--deep`. The exact sequence, once a
Team ID and signing identity are in place, is:

```bash
codesign --force --options runtime --timestamp --sign "Developer ID Application" \
  build/Maschine.app/Contents/MacOS/MaschineHost

codesign --force --options runtime --timestamp --sign "Developer ID Application" \
  build/Maschine.app/Contents/Library/SystemExtensions/MaschineMk3Dext.dext/Contents/MacOS/MaschineMk3Dext

codesign --force --options runtime --timestamp \
  --entitlements MaschineMk3Dext/MaschineMk3Dext.entitlements \
  --sign "Developer ID Application" \
  build/Maschine.app/Contents/Library/SystemExtensions/MaschineMk3Dext.dext

codesign --force --options runtime --timestamp \
  --entitlements MaschineHost/MaschineHost.entitlements \
  --sign "Developer ID Application" \
  build/Maschine.app
```

Distribution (`.pkg`, `notarytool submit --wait`, `stapler staple`)
lives in the P1 milestone and `docs/R2-packaging.md` §§4–5. It is not
part of this scaffold.
