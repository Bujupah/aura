# Aura on macOS — Platform Investigation

Status: v0.1, 2026-10-05. Investigated on macOS 27.0.1 (Apple silicon), Xcode toolchain Swift 6.3.

Each claim is tagged with how it is known:

- **[Doc]** — read from Apple's or the vendor's current documentation (links in §8)
- **[Tested]** — observed in this repository's build on this machine
- **[Reported]** — third-party reports, not an Apple statement
- **[To validate]** — a design assumption that Milestone 2 must prove with a spike

## 1. Summary

| Question | Answer |
| --- | --- |
| Can we capture system audio and the microphone as separate streams? | Yes, with ScreenCaptureKit on macOS 15+ **[Doc]** |
| Minimum macOS | **15.0**, set by microphone capture in ScreenCaptureKit **[Doc]** |
| Can an overlay float above full-screen meeting apps without stealing focus? | Yes, as a non-activating `NSPanel` **[Tested]** |
| Can we guarantee the overlay is invisible to screen sharing? | **No.** There is no supported API for that **[Doc]** |
| Do global shortcuts need Accessibility permission? | No for chords with modifiers **[Tested]**; yes for a bare-modifier push-to-talk **[To validate]** |

## 2. Audio capture

### 2.1 ScreenCaptureKit

`SCStream` delivers audio as `CMSampleBuffer`s on a per-output-type basis, which is exactly the separation Aura needs.

| API | Available | Use |
| --- | --- | --- |
| `SCStreamConfiguration.capturesAudio` | macOS 13.0 **[Doc]** | Turn on system audio; off by default |
| `SCStreamConfiguration.sampleRate`, `channelCount` | macOS 13.0 **[Doc]** | Request the delivery format |
| `SCStreamConfiguration.excludesCurrentProcessAudio` | macOS 13.0 **[Doc]** | Keep Aura's own sounds out of the meeting stream |
| `SCStreamOutputType.audio` | macOS 13.0 **[Doc]** | System-audio buffers → `CUSTOMER_OR_PARTICIPANT` |
| `SCStreamConfiguration.captureMicrophone`, `microphoneCaptureDeviceID` | macOS 15.0 **[Doc]** | Turn on microphone capture, choose the device |
| `SCStreamOutputType.microphone` | macOS 15.0 **[Doc]** | Microphone buffers → `SELLER` |

**Built** in `native/macos/AuraCapture` — compiled, linked and unit-tested for format conversion, but **not yet run against real devices** (see the README):

- One `SCStream` with two stream outputs (`.audio`, `.microphone`), each on its own queue. The streams are never mixed; both are stamped from the same host clock, which gives the monotonic ordering the event bus needs.
- ScreenCaptureKit is a screen-capture API, so a stream needs a content filter even when only audio is wanted. Aura uses a display filter with a 2×2, one-frame-per-second video configuration and attaches no screen output, so frames are never delivered. **[To validate]**: that the current OS accepts this configuration, and whether it logs dropped-frame noise.
- Aura's own application is excluded from that filter (brief §28, layer 2) and `excludesCurrentProcessAudio` is set, so Aura never captures itself.
- Each source has its own `AVAudioConverter`, which resamples to 24 kHz mono PCM16 before audio crosses into Rust and is rebuilt when the device format changes (for example, a headset connecting).
- **[To validate]**: whether system audio buffers keep arriving during silence. The Rust pacer does not depend on it — it fills silence itself.

### 2.2 Risks still to resolve on real devices

1. **Echo.** Without headphones, the customer's voice leaves the speakers and re-enters the microphone, so customer speech would be transcribed as `SELLER`. Options: voice-processing input (AEC) for the microphone path via `AVAudioEngine`, or suppressing mic segments that correlate with the system stream. This decides whether the mic comes from ScreenCaptureKit or `AVAudioEngine`. **[To validate]**
2. **Meeting apps' own processing.** Zoom and Teams apply their own echo cancellation and may switch the audio device or sample rate when a call starts. Device-change handling must be tested against real clients. **[To validate]**
3. **Bluetooth.** Opening the microphone on a Bluetooth headset can drop the output to a low-quality telephony profile. Aura must not be the reason call audio degrades; detect and warn. **[To validate]**
4. **A corrupted-recording report** exists on Apple's forums for `captureMicrophone` with file recording output. Aura streams sample buffers and does not use recording output, but the spike must check buffer integrity. **[Reported]**

### 2.3 Alternative: Core Audio process taps

`AudioHardwareCreateProcessTap` (macOS 14.2+ **[Doc]**) captures system or per-process audio without going through screen capture. It is the candidate replacement if the Screen Recording permission proves too costly for users (§3). Trade-off: more Core Audio plumbing (tap + aggregate device), and the microphone then comes from a separate API with its own clock to reconcile. Not chosen first because the brief specifies ScreenCaptureKit and a single stream gives both sources on one clock. Decision record: [ADR-002](../decisions/ADR-002-macos-audio-capture.md).

### 2.4 Processing after capture

Rust (`aura-audio`), fed over a C ABI from Swift: convert to mono PCM16, resample to the GPT-Live session rate (24 kHz — see [ai.md](ai.md)), level metering for the status bar, client-side VAD, bounded ring buffers with drop-oldest backpressure. Raw audio exists only in those buffers; there is no code path that writes it to disk unless a recording policy explicitly enables one.

## 3. Permissions

| Capability | Permission (TCC) | Usage string | Notes |
| --- | --- | --- | --- |
| System audio via ScreenCaptureKit | Screen & System Audio Recording | — (system prompt) | The largest adoption cost. Recent macOS versions also re-confirm ongoing capture access periodically and show a capture indicator **[To validate]** on 27 |
| Microphone | Microphone | `NSMicrophoneUsageDescription` | Required for either capture path |
| Global shortcuts with modifiers | none | — | **[Tested]** registered without any prompt |
| Bare-modifier push-to-talk (hold ⌥) | Input Monitoring or Accessibility | — | Needs an event tap; see §6 |

Operational notes:

- TCC grants attach to the code signature. Unsigned or ad-hoc-signed dev builds lose their grant on every rebuild and often fail to prompt at all **[Reported]**. Milestone 2 needs a stable Developer ID (or a consistent self-signed identity) for development.
- Distribution outside the App Store requires Developer ID signing, hardened runtime with the audio-input entitlement, and notarization. MDM can pre-approve some permissions for managed fleets, but **not** screen recording for standard users — it can only allow users to approve it. **[To validate]** against BMC's MDM.
- The overlay shows capture state at all times (brief §29); the OS indicators are a second, independent signal.

## 4. Overlay window behaviour

**Built in Milestone 1.** Both Aura windows are Tauri webview windows converted at startup to `NSPanel` subclasses via `tauri-nspanel`.

| Requirement | Mechanism | Status |
| --- | --- | --- |
| Always on top | Window level `NSStatusWindowLevel` (25) | **[Tested]** — CoreGraphics reports layer 25 |
| Visible on every Space and over full-screen apps | Collection behaviour `canJoinAllSpaces` + `fullScreenAuxiliary` + `stationary` + `ignoresCycle` | Set; see README for what was exercised |
| Never activates Aura / never steals focus from the meeting | `NSWindowStyleMaskNonactivatingPanel`; overlay panel returns `false` from `canBecomeKeyWindow` | Set |
| Palette receives typing without activating the app | Palette panel can become key; shown with `makeKeyWindow` | Set |
| No Dock icon, menu-bar only | Activation policy `.accessory` at runtime, `LSUIElement` in the bundle | **[Tested]** |
| Transparent, borderless, rounded | Tauri `transparent` + `decorations: false` (needs `macOSPrivateApi`), CSS radius | **[Tested]** |
| Draggable | `data-tauri-drag-region` on the status bar | Set |
| Click-through | `setIgnoresMouseEvents` via Tauri | Set |

Why `NSPanel` rather than a plain always-on-top `NSWindow`: only a non-activating panel can be clicked and typed into while another app stays frontmost, and it is the reliable way to appear over another app's full-screen Space.

`macOSPrivateApi` is required for transparent webview backgrounds. It uses private WebKit API and rules out the Mac App Store. Aura is distributed internally, so this is acceptable ([ADR-001](../decisions/ADR-001-desktop-framework.md)).

## 5. Screen sharing and presentation safe mode

### What the platform does

- `NSWindow.sharingType = .none` is the historical way to keep a window out of captures. Apple's current documentation calls it "a legacy constant that macOS no longer uses" and says not to use it to hide content from capture **[Doc]**.
- Capture built on ScreenCaptureKit includes such windows on macOS 15 and later; only older CoreGraphics capture paths still honour the flag **[Reported]** (tracked upstream as tauri-apps/tauri#14200, open).
- Aura sets the flag anyway (`contentProtected: true`). It costs nothing and still helps against legacy capture paths. CoreGraphics reports `sharingState = 0` for both Aura windows **[Tested]**. **It is not a guarantee and the product must never describe it as one.**

### What Aura does instead

| Layer | Behaviour | Status |
| --- | --- | --- |
| 1 | Best-effort content protection flag | **Built** |
| 2 | Exclude Aura's windows from Aura's own capture filter | **Built** |
| 3 | Presentation Safe Mode: collapse to nothing or a minimal indicator, move to a non-shared display when one exists, show responses only on request | Designed |
| 4 | Emergency hide, global ⌘⇧. — hides every Aura window, topic windows included; press again to restore | **Built** |
| 5 | Guidance to share a window or app rather than the whole display — Aura is then simply outside the shared content | Product copy |
| 6 | A `MeetingAppIntegration` interface for per-app share detection (Teams, Zoom, Meet, Slack) | Interface only, later |

All of this applies equally to the per-topic note windows, which are created with the same flag and are panels of the same kind. A window opened while hidden stays hidden.

Emergency hide is enforced in the state machine: while hidden, no command except restore is honoured, so nothing — not a recommendation, not the palette — can put a window on screen.

Detecting that *another* app is sharing the screen has no public API. Layer 6 would rely on per-app heuristics, which is why it is an abstraction and not a promise.

## 6. Global shortcuts

Implemented with `tauri-plugin-global-shortcut` (Carbon hot keys underneath). Chords with at least one modifier register without any permission **[Tested]**.

| Default | Action | Status |
| --- | --- | --- |
| ⌥Space | Command palette | **Built** |
| ⌘⇧. | Hide / restore all Aura windows | **Built** |
| ⌥⇧Space | Expand / collapse overlay | **Built** |
| ⌘⇧E | Engineer Mode | Reserved (Milestone 9) |

- ⌥Space is also the default of other launchers. Registration can fail when another app owns a chord; Aura records which shortcuts registered and shows unavailable ones in the overlay instead of failing silently. User-configurable bindings come with the settings store.
- **Push-to-Aura (hold ⌥ alone)** cannot be a hot key: a bare modifier is only observable through an event tap or a global `flagsChanged` monitor, which requires Input Monitoring or Accessibility permission. Recommendation: ship push-to-talk on a modifier chord first, and offer bare-⌥ as an opt-in that explains the extra permission. **[To validate]**

## 7. Tauri ↔ native integration

- Milestone 1 needs no Swift: panels are configured from Rust through `objc2` bindings.
- `native/macos/AuraCapture` (**built**) is a Swift package compiled by `aura-audio`'s build script (`swift-rs`) into a static library and linked into the Tauri binary, exposing a small C ABI (`aura_capture_start`, `aura_capture_stop`, a PCM callback, a state callback). `stop` blocks until no callback can run, so the Rust side can free its handler safely. The Swift runtime is found through an rpath set in `.cargo/config.toml`. In-process keeps one bundle, one signature and one set of TCC grants.
- An out-of-process helper (XPC) is the fallback if capture ever needs to be isolated from the webview process for crash containment. Not needed yet.

## 8. Sources

Apple documentation (read 2026-10-05):

- [SCStreamConfiguration.capturesAudio](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio)
- [SCStreamConfiguration.excludesCurrentProcessAudio](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/excludescurrentprocessaudio)
- [SCStreamConfiguration.captureMicrophone](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturemicrophone)
- [SCStreamOutputType.audio](https://developer.apple.com/documentation/screencapturekit/scstreamoutputtype/audio), [.microphone](https://developer.apple.com/documentation/screencapturekit/scstreamoutputtype/microphone)
- [SCContentSharingPicker](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker)
- [NSWindow.SharingType.none](https://developer.apple.com/documentation/appkit/nswindow/sharingtype-swift.enum/none)
- [AudioHardwareCreateProcessTap](https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:))

Other:

- [tauri-apps/tauri#14200 — ScreenCaptureKit ignores content protection on macOS 15+](https://github.com/tauri-apps/tauri/issues/14200)
- [Apple Developer Forums — corrupted output with captureMicrophone](https://developer.apple.com/forums/thread/805892)
- [Tauri global-shortcut plugin](https://v2.tauri.app/plugin/global-shortcut/)
- [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel)
