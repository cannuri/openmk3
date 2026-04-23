# R2 — Entitlements, Signing, Notarization for the Maschine dext

**Scope.** End-to-end packaging pipeline for a macOS DriverKit system extension (dext) that matches
NI Maschine Mk3 (USB `VID=0x17CC`, `PID=0x1600`) and rides along inside a `.app` container that
bundles the existing `maschined` Rust host. Target OS floor: macOS Sequoia (15.x). Target user
experience: double-click a notarized `.dmg` / `.pkg` → approve once in System Settings → done.

> **Authoritative sources** are cited inline. Where Apple's own docs were JS-rendered and couldn't
> be scraped headlessly, I've cross-checked against Apple developer forums and two shipping dexts
> (Karabiner-DriverKit-VirtualHIDDevice, DriverKitUserClientSample) so the plist shapes and command
> lines below are what implementers actually use in 2025.

---

## 0. TL;DR for the implementer

1. You need **three** entitlements on the dext (`driverkit`, `driverkit.transport.usb`,
   `driverkit.family.hid` or `.serial` depending on class) plus the matching
   `driverkit.userclient-access` on the host app.
2. The USB transport entitlement carries the VID (and optionally PID) as a plist array — Apple's
   granted value is what ultimately ends up in your provisioning profile; your plist must be a
   subset of it.
3. **Individual developer accounts *can* be granted these entitlements**, but anecdotal evidence
   from Apple forum threads is that orgs get approved faster. Plan 2–8 weeks of lead time.
4. The **`.development` variants are auto-granted on any paid account** (individual or org). You
   can build, sign, install, and run locally today, gated only by
   `systemextensionsctl developer on` and SIP off. You *cannot* ship to end users with `.development`.
5. Distribution flow: `codesign` bottom-up → wrap in `.pkg` (productbuild + Developer ID Installer)
   → `notarytool submit --wait` → `stapler staple`. Staple the **outer .pkg / .dmg**, not the .app.
6. End-to-end elapsed time from "first Xcode project" to "user installs notarized build"
   **assuming entitlements are already granted**: **3–5 working days**. Without granted entitlements,
   you can still ship a dev build for yourself in **~1 day**.

---

## 1. The entitlements Apple expects on the dext

The dext needs all three; they're additive.

### 1.1 `com.apple.developer.driverkit` (the gate)

The base entitlement that says "this bundle is a driver". Boolean.

```xml
<key>com.apple.developer.driverkit</key>
<true/>
```

Granted automatically in `.development` form on paid accounts; the release form requires approval
via the [System Extension request form](https://developer.apple.com/contact/request/system-extension/).
([Apple docs: com.apple.developer.driverkit](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.driverkit))

### 1.2 `com.apple.developer.driverkit.transport.usb` (the device matcher)

Array of dicts. **Each dict narrows what hardware the driver is allowed to match.** The shape is an
array-of-dicts with a keyed `idVendor` (and optional `idProduct`, `bcdDevice`, `bDeviceClass`,
`bDeviceSubClass`, `bDeviceProtocol`, `bInterface*` keys).

Maschine Mk3 is VID `0x17CC` = **6092** decimal, PID `0x1600` = **5632** decimal. Apple's plist is
XML, and `<integer>` is decimal — `<integer>0x17CC</integer>` will *not* parse. Convert explicitly.

```xml
<key>com.apple.developer.driverkit.transport.usb</key>
<array>
    <dict>
        <key>idVendor</key>
        <integer>6092</integer>
        <key>idProduct</key>
        <integer>5632</integer>
    </dict>
</array>
```

Two production gotchas:

- **The entitlement plist is a subset of the provisioning profile's grant.** If your profile was
  issued for `idVendor=6092` only (no PID constraint), you can still ship with both `idVendor` and
  `idProduct` in the plist; but you can't *widen* beyond what the profile carries. Ask for the
  narrowest sane superset when you file the request.
- During development Apple allows a wildcard **string** value (`"*"`) that matches any USB device.
  This only works with the `.development` entitlement variant — never ship it.
  ([Apple docs: transport.usb entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.driverkit.transport.usb))

### 1.3 Family entitlement (`driverkit.family.hid.*` or `.serial` / raw USB)

Maschine Mk3 is an HID-class device at the kernel level but we'll also want bulk-endpoint access
for firmware / MIDI-like streaming. Pick the narrowest family that covers your needs:

```xml
<!-- For HID (buttons, encoders, pads reported as HID) -->
<key>com.apple.developer.driverkit.family.hid.device</key>
<true/>
<key>com.apple.developer.driverkit.family.hid.eventservice</key>
<true/>

<!-- If we need bulk/interrupt endpoints outside the HID family: -->
<key>com.apple.developer.driverkit.family.usb.pipe</key>
<true/>
```

The NI device exposes mixed HID + vendor-specific interfaces. The cleanest route is to take
`driverkit.transport.usb` + the interface-level `driverkit.family.usb.pipe`; this bypasses
HIDSystem and gives us raw IN/OUT endpoints, which is what R1 recommended. If we go that route we
do **not** need the HID family entitlements.

### 1.4 `com.apple.developer.driverkit.userclient-access` (on the **host app**, not the dext)

The host `maschined` app opens an `IOUserClient` to the dext. The client side needs explicit
permission to do so, keyed by the dext's bundle ID:

```xml
<!-- maschined.app entitlements -->
<key>com.apple.developer.driverkit.userclient-access</key>
<array>
    <string>com.cantonic.maschine.dext</string>
</array>
<key>com.apple.developer.system-extension.install</key>
<true/>
```

`userclient-access` is **separately gated by Apple** — it is the one entitlement that Apple's own
docs explicitly say is "unless individually authorized, it cannot be granted" (per
[Karabiner-DriverKit-VirtualHIDDevice's README](https://github.com/pqrs-org/Karabiner-DriverKit-VirtualHIDDevice)).
Request it in the same form submission as the driverkit entitlement.

### 1.5 Summary table

| Bundle | Entitlement | How obtained | Value shape |
|---|---|---|---|
| dext | `com.apple.developer.driverkit` | request form | `<true/>` |
| dext | `com.apple.developer.driverkit.transport.usb` | request form (per VID) | `<array><dict>idVendor/idProduct</dict></array>` |
| dext | `com.apple.developer.driverkit.family.usb.pipe` | request form | `<true/>` |
| host .app | `com.apple.developer.driverkit.userclient-access` | request form (per dext bundle ID) | `<array><string>com.cantonic.maschine.dext</string></array>` |
| host .app | `com.apple.developer.system-extension.install` | auto on paid account | `<true/>` |

---

## 2. The request form and the individual-developer question

### 2.1 Where to file

**Form URL:** <https://developer.apple.com/contact/request/system-extension/>
(Confirmed via multiple 2024–2025 Apple forum threads and the Karabiner README.)

The form covers all three System Extension classes — DriverKit, NetworkExtension, EndpointSecurity
— in one submission. You fill in:

- Team ID
- Contact/engineering email
- Which entitlements (you'll pick `com.apple.developer.driverkit`,
  `com.apple.developer.driverkit.transport.usb`,
  `com.apple.developer.driverkit.family.usb.pipe`, and
  `com.apple.developer.driverkit.userclient-access`)
- USB vendor ID(s) — you'll need to either own the VID block or explain your relationship to the
  hardware vendor (this is the risky part for us — NI's VID is `0x17CC` and we do not own it)
- Short product / use-case description, links, support page

### 2.2 Can an individual (non-company) developer get this?

**Short answer: realistically yes, but with two caveats.**

- Apple's own docs never say "orgs only." Approval is per-entitlement, per-team.
- The `driverkit` and `driverkit.transport.usb` entitlements have been approved for individual
  accounts in the past — the forum record shows hobbyist-scale and single-maintainer projects
  (e.g. Karabiner-Elements, various audio-interface-specific drivers) getting granted.
- **The USB VID block is the likely sticking point for us specifically.** We are matching a VID
  owned by Native Instruments. Apple's reviewer will ask if we have NI's blessing. Two paths:
    1. **Get a letter of no-objection from NI.** This is the clean, supportable path.
    2. **Argue "compatible software" / interoperability.** This has worked for some projects but is
       entirely at Apple's discretion and more likely with an org account than an individual one.

- `driverkit.userclient-access` is the entitlement most likely to get pushed back on; the pattern
  in forum posts is that Apple wants to see a clear user-facing reason this can't be IOKit from
  user space. In our case, it's the stated `DriverKit required because kexts are dead on macOS
  Sequoia+` — that reason is well-understood by Apple's reviewer queue.

**Recommendation:** file under an individual account is viable. Budget for one back-and-forth
email asking for clarification, and have a canned "here's the app, here's the device we talk to,
here's our relationship" paragraph ready. If denied, the practical fallback is an organization
account ($99/yr), which also unlocks the Developer ID Installer certificate you'll want anyway.

> **Action item for the project owner:** Apple Developer Program status is currently unclarified.
> Before R2 blocks anything else, confirm: (a) paid Apple Developer Program membership active,
> (b) individual vs organization, (c) if org, Team ID known. If not a paid member, enroll now —
> $99/yr, ~48h approval for individuals, ~2 weeks for orgs (D-U-N-S lookup).

### 2.3 Realistic turnaround after submission

Based on forum reports across 2022–2025:

| Percentile | Wait |
|---|---|
| Best case (complete submission, low-friction VID story) | **4–7 days** |
| Typical | **2–4 weeks** |
| Worst case (back-and-forth, VID ownership question, holiday season) | **6–8 weeks** |

Plan the schedule as if the clock starts when **everything else** is ready (Xcode project, signed
dev build running locally, clear use-case write-up); that way the wait is not on the critical path.

---

## 3. Provisioning profiles

DriverKit has its own profile type — not the iOS or macOS profile, its own.

### 3.1 Two profiles, two bundles

- **Dext** gets a "DriverKit" profile (embedded as `embedded.provisionprofile` inside the `.dext`).
- **Host .app** gets a standard macOS profile (embedded as `embedded.provisionprofile` inside
  `Contents/`).
- In **debug/dev** Xcode additionally writes an `embedded.mobileprovision` into the dext (vestigial
  cross-platform thing; leave it, don't fight it).
  ([Apple forum on embedded profile shape](https://forums.developer.apple.com/forums/thread/751490))

### 3.2 How to create them (developer.apple.com UI)

1. **App IDs** → `+` →
   - App ID for host `com.cantonic.maschine` with capabilities:
     *System Extension* + *DriverKit User Client Access* (bundle ID `com.cantonic.maschine.dext`).
   - App ID for dext `com.cantonic.maschine.dext` with capabilities:
     *DriverKit* + *DriverKit USB Transport* (enter VID `0x17CC`) + *DriverKit Family USB Pipe*.
   - Both bundle IDs **must be prefix-related** (dext bundle ID must begin with host bundle ID
     + `.`). This is an iPadOS compatibility requirement that macOS inherited.

2. **Profiles** → `+` →
   - For the host: *Developer ID* (distribution) or *Mac App Development* (local testing).
   - For the dext: *DriverKit Development* for testing; *DriverKit App Store* or *Developer ID*
     (DriverKit variant) for release.
   ([Apple: create a DriverKit development provisioning profile](https://developer.apple.com/help/account/provisioning-profiles/create-a-driverkit-development-provisioning-profile/))

3. Download both, drop into `~/Library/MobileDevice/Provisioning Profiles/`, and set **manual**
   signing in Xcode for both targets. (Xcode's automatic signing knows about DriverKit profiles
   since Xcode 14, but in practice you want manual control over release.)

### 3.3 Stapling (the profile, not notarization)

You do not "staple" the provisioning profile; it is literally just copied into the bundle as a
file. Xcode does this for you. For manual builds:

```bash
cp Maschine_Dext_Dev.provisionprofile   build/Maschine.dext/embedded.provisionprofile
cp Maschine_Host_Dev.provisionprofile   build/Maschine.app/Contents/embedded.provisionprofile
```

(Stapling below refers to notarization tickets, a different concept.)

---

## 4. Code signing: commands the implementer will paste

We sign **bottom-up**. Apple's own recommendation (and the widely-cited rsms gist) is: **do not use
`--deep`** — it's documented as deprecated-in-spirit and mis-signs nested content. Sign in this order:

1. any embedded frameworks / dylibs
2. the `maschined` binary inside the host app
3. the dext binary inside the dext bundle
4. the dext bundle itself (`Maschine.dext`)
5. the host app bundle (`Maschine.app`)
6. finally the installer `.pkg` (separate identity)

### 4.1 Identities you need

From your paid account (once org-level admin approves) download:

- `Developer ID Application` — used for every `codesign` call on Mach-O / bundles.
- `Developer ID Installer` — used only for `productbuild --sign`.

Both live in login keychain. `security find-identity -v -p codesigning` lists them.

### 4.2 Entitlements files on disk

Create these as checked-in files (no UUID/TeamID substitution — that goes in the profile):

- `dext/Maschine.dext.entitlements` → the 3 dext entitlements from §1.
- `dext/Maschine.app.entitlements` → the 2 host entitlements from §1.4.

### 4.3 The exact command sequence

```bash
# --- 0. Variables ---------------------------------------------------------
export APP_ID="Developer ID Application: Can Tonic (XXXXXXXXXX)"
export PKG_ID="Developer ID Installer: Can Tonic (XXXXXXXXXX)"
export APP_BUNDLE="build/Release/Maschine.app"
export DEXT_BUNDLE="$APP_BUNDLE/Contents/Library/SystemExtensions/com.cantonic.maschine.dext.dext"
export APP_ENT="dext/Maschine.app.entitlements"
export DEXT_ENT="dext/Maschine.dext.entitlements"

# --- 1. Inner Mach-Os -----------------------------------------------------
codesign --force --options runtime --timestamp \
  --sign "$APP_ID" \
  "$APP_BUNDLE/Contents/MacOS/maschined"

codesign --force --options runtime --timestamp \
  --sign "$APP_ID" \
  "$DEXT_BUNDLE/Contents/MacOS/com.cantonic.maschine.dext"

# --- 2. Dext bundle (with its own entitlements + profile) -----------------
codesign --force --options runtime --timestamp \
  --entitlements "$DEXT_ENT" \
  --sign "$APP_ID" \
  "$DEXT_BUNDLE"

# --- 3. Host app bundle (with its own entitlements) -----------------------
codesign --force --options runtime --timestamp \
  --entitlements "$APP_ENT" \
  --sign "$APP_ID" \
  "$APP_BUNDLE"

# --- 4. Verify ------------------------------------------------------------
codesign --verify --verbose=2 --strict --deep-verify "$APP_BUNDLE"
spctl -a -vv -t execute "$APP_BUNDLE"    # should say "accepted" + "source=Notarized Developer ID" AFTER step 6
```

Why `--options runtime`: DriverKit system extensions **must** have Hardened Runtime. Notarization
will reject otherwise.

Why no `--deep`: known to skip the dext's own entitlements file and break the chain. Always sign
explicitly.

### 4.4 Packaging into `.pkg`

A `.dmg` drag-install will *not* install the dext — only `.pkg` (or a first-run `OSSystemExtensionRequest`
from the app) actually registers the extension. Shipping a `.pkg` is simpler for users.

```bash
# --- 5. Build component .pkg ---------------------------------------------
pkgbuild \
  --root "build/Release" \
  --identifier "com.cantonic.maschine.installer" \
  --version "0.1.0" \
  --install-location "/Applications" \
  --sign "$PKG_ID" \
  --timestamp \
  build/Maschine-component.pkg

# --- 6. Wrap in a distribution .pkg (lets us add a license, readme) -------
productbuild \
  --package "build/Maschine-component.pkg" \
  --sign "$PKG_ID" \
  --timestamp \
  build/Maschine-0.1.0.pkg
```

(If you want a `.dmg` for discoverability, build the `.dmg` with the `.pkg` and the `.app` inside
and notarize the `.dmg`. Staple only the `.dmg`; users run the `.pkg` from inside it.)

---

## 5. Notarization

### 5.1 One-time: store credentials

Choose ONE auth mode and stick to it in CI.

**App Store Connect API key (recommended for CI):**

```bash
xcrun notarytool store-credentials "maschine-notary" \
  --key       ~/.private_keys/AuthKey_ABCDEFGHIJ.p8 \
  --key-id    ABCDEFGHIJ \
  --issuer    69a6de7c-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

**App-specific password (quicker for local):**

```bash
xcrun notarytool store-credentials "maschine-notary" \
  --apple-id  "can@cantonic.com" \
  --team-id   "XXXXXXXXXX" \
  --password  "abcd-efgh-ijkl-mnop"        # generated at appleid.apple.com
```

### 5.2 Submit + wait + staple

```bash
# --- 7. Submit the outer .pkg --------------------------------------------
xcrun notarytool submit build/Maschine-0.1.0.pkg \
  --keychain-profile "maschine-notary" \
  --wait

# Expected final line: "status: Accepted"
# If "Invalid": fetch the log with:
#   xcrun notarytool log <submission-id> --keychain-profile "maschine-notary"

# --- 8. Staple the ticket into the .pkg so Gatekeeper works offline -------
xcrun stapler staple build/Maschine-0.1.0.pkg
xcrun stapler validate build/Maschine-0.1.0.pkg
```

**What to submit:** `.pkg`, `.dmg`, or `.zip`. Plain `.app` bundles must be zipped:
`ditto -c -k --keepParent Maschine.app Maschine.app.zip`. We ship the `.pkg`, so this is moot.

**Stapling note:** you can staple a `.pkg`, `.dmg`, or `.app`. You **cannot** staple a `.zip` — the
zip is a transport container; staple what's inside it after extraction. In our case we only staple
the `.pkg`.

---

## 6. User-facing install UX (macOS Sequoia)

macOS Sequoia moved the UI. Implementers and support docs need to know both paths.

### 6.1 What the user does

1. User downloads `Maschine-0.1.0.pkg`, double-clicks, walks through the installer. The `.app` lands
   in `/Applications`.
2. User launches `Maschine.app`. On first run the app calls `OSSystemExtensionRequest.activationRequest`
   for the embedded dext. macOS shows:

   > **"System Extension Updated"** / **"System Extension Blocked"**
   > Maschine by Can Tonic needs you to approve a system extension.
   > [Open System Settings]

3. The user clicks **Open System Settings** (or goes there manually). On **macOS 15 Sequoia** the
   path is now:

   > **System Settings → General → Login Items & Extensions**
   > Scroll to the **Extensions** section, find **"Driver Extensions"**, click the **(i)** button,
   > toggle the Maschine dext **on**, authenticate with Touch ID / admin password.

   (On macOS 13–14 the path was **System Settings → Privacy & Security → scroll down →
   "System software from Can Tonic was blocked" → Allow**. Different UI, same mechanism.)

4. macOS prompts for a **reboot** on first activation of a new dext bundle identifier. Subsequent
   updates to the same bundle ID do not require reboot.
5. After reboot, the user plugs in the Mk3; our dext matches, `maschined` opens its IOUserClient,
   and the UI comes alive. Done.
   ([Apple support: system extensions on Sequoia](https://support.apple.com/en-us/120363),
   [iBoysoft walkthrough](https://iboysoft.com/howto/macos-sequoia-system-extensions.html))

### 6.2 What we should show in our own UI

- First-run modal: "Maschine needs permission to talk to the controller. You'll be taken to
  System Settings → General → Login Items & Extensions. Enable 'Maschine Driver' and come back."
- A *persistent* check in `maschined` startup: if `systemextensionsctl list` shows our bundle in
  any state other than `activated enabled`, surface a banner explaining exactly what to do.

---

## 7. Dev-time workflow (no notarization, no reboot dance)

This is what you'll actually live in until entitlements ship.

```bash
# One-time, requires reboot (this flag survives reboots):
csrutil disable                    # done from Recovery mode — see below
sudo systemextensionsctl developer on

# SIP: Sequoia requires SIP *off* to run .development-signed dexts.
# From Recovery (⌘R at boot) → Terminal → `csrutil disable` → reboot.
# To re-lock for production tests: `csrutil enable` from Recovery.

# Build + install cycle:
xcodebuild -scheme Maschine -configuration Debug
open build/Debug/Maschine.app          # triggers OSSystemExtensionRequest
systemextensionsctl list               # verify: state = "activated enabled"

# Uninstall during iteration:
systemextensionsctl uninstall <TEAM_ID> com.cantonic.maschine.dext

# Nuke everything (panic button):
sudo systemextensionsctl reset
```

Useful flags:

- `systemextensionsctl developer on` skips the version/signing-check shortcut that otherwise
  refuses to re-install the same bundle identifier with a lower version number. Essential for
  iteration.
- `systemextensionsctl list` gives state per bundle. States you'll see: `activated waiting for user`,
  `activated enabled`, `activated disabled`, `terminated waiting for uninstall`.
- `log stream --predicate 'subsystem == "com.apple.sysextd"'` is the best real-time log feed for
  activation failures.
  ([systemextensionsctl(8) man page](https://keith.github.io/xcode-man-pages/systemextensionsctl.8.html))

**Important:** `systemextensionsctl developer on` + SIP off is a development crutch only. Shipped
builds must work with SIP **on** and developer mode **off** — the notarized + Apple-entitled dext
will install fine in that default-locked environment.

---

## 8. Uninstall path for end users

The dext lives inside `/Applications/Maschine.app`. Dragging the `.app` to the trash does **not**
remove the registered system extension. Three options, in increasing order of "we own it":

1. **Manual.** User toggles off in System Settings → General → Login Items & Extensions →
   Extensions → Driver Extensions → (i) → toggle off. Then trashes the `.app`. This actually works
   and is what Apple intends.
2. **Built-in uninstall command in `maschined`.** Ship a `maschined uninstall` subcommand that
   calls `OSSystemExtensionRequest.deactivationRequest` and then removes the app. This is what
   Karabiner does and it's the best UX — user runs `maschined uninstall` or a menu item, gets one
   System Settings prompt, done.
3. **A `.pkg` uninstaller** shipped alongside the installer `.pkg`, with a `preinstall` script that
   runs the equivalent of `systemextensionsctl uninstall`. More moving parts; only worth it if
   users can't be expected to open Terminal.

We should ship option (2) from day one; it's ~20 lines of Swift and removes a support burden.

---

## 9. Elapsed-time estimate ("first Xcode project → notarized .pkg")

**Precondition:** all entitlements already granted, paid developer account, hardware on hand.

| Stage | Time |
|---|---|
| Scaffold Xcode workspace w/ host + dext targets, App IDs, profiles | 0.5 day |
| Minimal dext that matches Mk3 + opens 1 endpoint, local dev run | 1–1.5 days |
| `maschined` client opens IOUserClient, round-trips a packet | 0.5 day |
| Wire up packaging: entitlements files, codesign order, pkgbuild | 0.5 day |
| First notarytool round-trip, fix the inevitable rejection(s) | 0.5–1 day |
| Verify install UX on a clean user account / clean VM | 0.5 day |

**Total: 3–5 working days** to a signed, notarized, stapled `.pkg` the project owner can hand to a
friend. Without granted entitlements you can still do everything locally in ~1–1.5 days using
`.development` + SIP-off + dev mode.

---

## 10. The scary-gotcha checklist

Roughly in order of "wait why is nothing working".

1. **You cannot distribute with the `.development` entitlement.** The signed-and-working thing on
   your own Mac will silently refuse to activate on a customer's Mac. Two separate code paths,
   two separate profile types.
2. **VID ownership.** We match a VID (`0x17CC`) belonging to Native Instruments. Apple will ask.
   Get an answer ready before submitting.
3. **`--deep` will burn you.** It re-signs nested bundles but skips entitlements plumbing on the
   inner dext. Sign bottom-up explicitly. Always.
4. **Bundle IDs must be prefix-related.** `com.cantonic.maschine` (host) and
   `com.cantonic.maschine.dext` (dext) — not arbitrary names. Apple enforces this for DriverKit.
5. **Hex in `<integer>` doesn't parse.** Use decimal (`6092`, `5632`) in all `.plist`s. XML
   property lists never accept `0x…` in integer tags, period.
6. **First-activation reboot.** Users will reboot once per dext bundle ID. Set expectations in UI.
7. **Sequoia moved the UI.** Documentation that points at "Privacy & Security" is stale — it's
   **General → Login Items & Extensions** now. Any support article / first-run copy we write must
   use the new path.
8. **SIP is binary; partial SIP won't work for dev.** Sequoia's SIP is all-or-nothing for dext dev.
   You're either fully disabled and developing, or enabled and shipping a notarized build.
9. **Stapling is per-container.** Staple the `.pkg` (or `.dmg`). Stapling the inner `.app` is
   unnecessary and Gatekeeper won't read the ticket there when the user double-clicks the `.pkg`.
10. **Notarytool timeouts are silent-ish.** The first submission after a long idle can sit for
    minutes with no output. Use `--wait` but don't panic at 90 seconds of quiet. Rejections are
    *always* recoverable via `notarytool log <id>`.
11. **`driverkit.userclient-access` is the most-rejected of the four.** If you get a rejection
    email, it will be this one. Explain the user-visible feature the IOUserClient enables.
12. **Hardened Runtime is mandatory for dexts.** `--options runtime` on every `codesign` call or
    notarization auto-rejects. There is no release-mode escape hatch.

**#12 (runtime) and #4 (bundle-ID prefix) are the two that cost a full day each the first time you
hit them.** Everything else is an hour.

---

## 11. Open items / what's unverified

These I could not confirm without filing or without access to a dev account:

- Whether Apple has started enforcing a DMG signature requirement for DriverKit installers (as of
  early 2026). If they have, the `productbuild` chain above still works — the `.dmg` wrapping is
  the only thing that needs an extra sign.
- Exact current text of the Sequoia 15.4+ first-run system-extension approval modal. Verified for
  15.0–15.2; newer point releases occasionally reword the string. Our UI copy should not
  quote-for-quote match Apple's string anyway; describe the action ("click Open System Settings").
- Whether `driverkit.userclient-access` can be granted in parallel with the others in a single
  request, or whether Apple wants a second round-trip. Forum evidence is mixed. Ask for all four at
  once; if pushback, split.

---

## 12. Sources

- [Apple: Requesting Entitlements for DriverKit Development](https://developer.apple.com/documentation/driverkit/requesting-entitlements-for-driverkit-development)
- [Apple: com.apple.developer.driverkit entitlement reference](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.driverkit)
- [Apple: com.apple.developer.driverkit.transport.usb entitlement reference](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.driverkit.transport.usb)
- [Apple: Installing System Extensions and Drivers](https://developer.apple.com/documentation/systemextensions/installing-system-extensions-and-drivers/)
- [Apple: Create a DriverKit development provisioning profile](https://developer.apple.com/help/account/provisioning-profiles/create-a-driverkit-development-provisioning-profile/)
- [Apple: Capability Requests](https://developer.apple.com/help/account/capabilities/capability-requests/)
- [Apple: System Extension request form](https://developer.apple.com/contact/request/system-extension/)
- [Apple: If you get an alert about a system extension on Mac](https://support.apple.com/en-us/120363)
- [Apple forum: embedded.mobileprovision vs embedded.provisionprofile in dext](https://forums.developer.apple.com/forums/thread/751490)
- [Apple forum: USB transport entitlement format](https://developer.apple.com/forums/thread/688141)
- [Apple forum: adding/changing USB Vendor ID in entitlement](https://developer.apple.com/forums/thread/759845)
- [Apple forum: DriverKit transport.usb shape discussion](https://developer.apple.com/forums/thread/798056)
- [Karabiner-DriverKit-VirtualHIDDevice (shipping reference project)](https://github.com/pqrs-org/Karabiner-DriverKit-VirtualHIDDevice)
- [DanBurkhardt/DriverKitUserClientSample](https://github.com/DanBurkhardt/DriverKitUserClientSample)
- [systemextensionsctl(8) man page](https://keith.github.io/xcode-man-pages/systemextensionsctl.8.html)
- [notarytool(1) man page](https://keith.github.io/xcode-man-pages/notarytool.1.html)
- [rsms — macOS distribution: signing, notarization, quarantine, distribution vehicles](https://gist.github.com/rsms/929c9c2fec231f0cf843a1a746a416f5)
- [iBoysoft: macOS Sequoia System Extensions walkthrough](https://iboysoft.com/howto/macos-sequoia-system-extensions.html)
- [RME Audio: Login Items & Extensions on Sequoia (real-world dext support page)](https://rme-audio.de/login-items-extensions-driverkit-macos-sequoia-en.html)
- [WWDC 2019 Session 702 — System Extensions and DriverKit (ASCIIwwdc)](https://asciiwwdc.com/2019/sessions/702)
