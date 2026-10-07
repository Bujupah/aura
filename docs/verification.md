# Verification status

| Check | Result |
| --- | --- |
| `cargo test --workspace` | 89 passing (shell, turn assembly, arrangement validation, screen layout, pacing, resampling, level meter, GPT-Live and Live Translate protocols, note-taking agent, illustrator, settings, prompts) |
| `swift test` in `native/macos/AuraCapture` | 5 passing (format conversion to mono PCM16 at 24 and 16 kHz) |
| `pnpm test` | 20 passing (palette, transcript ordering, note labels, translation summary) |
| GPT-Live with a real session | 12.7 s synthetic clip transcribed word-for-word at ~0.4 s lag ([details](architecture/ai.md#31-what-a-real-session-showed)) |
| Two-stream session from audio fixtures, in the terminal probe and in the real app | Both speakers transcribed, labelled and ordered correctly; sessions closed with confirmed usage |
| Overlay: listening state, meters, transcript, start/stop | Checked in a browser against the dev bridge |
| Agent-arranged topic windows in the real app, from a two-part fixture with web access on | The agent opened three windows in three different corners at three sizes, then added a fourth; it searched the web for the customer's product question. Each update took 5.3–8.3 s after a turn ended. An earlier build was also checked against emergency hide and restore |
| Diagrams | On a clip where the customer described their incident process, the agent drew a correct six-node flowchart unprompted; the real app opened it in a tall window. Rendering, and the fallback for invalid diagram source, checked in a browser |
| Illustrator sub-agent | One live run: a clean labelled image in 15.9 s. It added an arrow between two systems that nobody had described — images can invent relationships, which is why they are captioned and diagrams are preferred. The path from a finished image into a real window has not been exercised: the agent chose diagrams in every test run |
| Window design | All window kinds reviewed side by side in a dev gallery (`pnpm dev:web`, then `?window=gallery`) |
| Translation, incoming | Live: English speech became accurate Spanish and French text and speech. In the real app, a Spanish-speaking customer appeared as English turns and English topic notes while the seller's stream stayed on GPT-Live |
| Translation, outgoing | Only the refusal path: with no virtual microphone installed the app declines to start and explains. Audio playback itself was checked silently on the default output |
| Closing topic windows | Dismissing a window and starting a new session both close windows correctly on real windows. Builds before this one crashed when a topic window closed, and left old windows behind on a new session; both are fixed |
| Agent response time | Scripted six-turn conversation, one run each: restating every window took 2.8–7.7 s per update (mean 4.8 s); letting the agent keep unchanged windows by id took 1.9–5.4 s (mean 3.9 s). Turning reasoning effort off was faster still (mean 2.9 s) but merged everything into one window and lost the diagram, so it was not adopted |
| Windows stay where the seller puts them | In the real app a topic window was moved the way a drag would move it; the app pinned it, and after the agent's next update (which added a second window) it was still at the same position. Done with a scripted move, not a real mouse drag |
| Only the seller removes topics | Core rules tested: unmentioned windows stay, order is stable, removal deletes the topic. The path from the ✕ button through to the agent being told has not been exercised live |
| Shell windows: expand, palette, hide all, restore; frontmost app unchanged | Observed on real windows |
| `Aura.app` bundle builds with the capture library linked and the microphone usage string | Observed |

**Not verified — needs you:**

- **Real capture.** The ScreenCaptureKit path has never run against a real microphone or real system audio: this build environment has neither permission, and granting them is yours to do. The code compiles, links and its format conversion is tested, but the first press of *Start listening* is its first real run.
- Pressing the physical shortcuts, the tray menu, dragging, and clicking controls in the real windows (I could only drive them through the app's internal command path).
- Appearance over a full-screen app.

Known gaps: the agent's updates are slow (5–8 s) and its judgement is untuned — it sometimes searches when it need not and groups facts loosely; moving or resizing an already-open window has not been observed on real windows yet; web sources are shown as site names, not clickable links; diagrams and images cannot be enlarged yet; the Gemini key in `.env` is on the free tier, which has no image quota, so images use GPT; no echo handling; no reconnect if the network drops mid-session (the session ends with an error); a turn's final punctuation mark is sometimes missing; overlay position is not remembered; shortcuts are not configurable; no push-to-Aura yet.

This file is the honest ledger. If something is not listed as checked, assume it has not been.
