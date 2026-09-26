# Decision panel: q8_windows_evidence_gate

_Raw research, options, judge verdicts and verification for this question (2026-09-25)._

## research

```json
{
  "code_facts": [
    "Recommendation (my reasoning, not a verified result): do NOT make live Japanese-IME + Narrator evidence an H0 gate for the plugin seam. Split it into three parts. (1) An H0 contract gate that runs in CI: the D10 pull text-store trait gets a headless conformance kit, plus TSF unit tests against a mock ITextStoreACP/thread manager, the way Chromium tests its TSFTextStore. (2) An H0/B1 automated Windows lane: the UIA part of `cargo xtask device windows-a11y` runs as a CI job on windows-latest (AccessKit already runs this kind of test there), and a new `cargo xtask device windows-ime` runs on the maintainer host or in a VM. (3) The human live session (Japanese IME typing 東京 via 'toukyou', plus Narrator reading it) stays in exit B1. Windows 'beta' status then carries a freshness rule: the record must be re-run when paths under crates/flui-platform/src/platforms/windows/** or text-input/semantics change since the recorded commit, and in any case before each release tag. The rule is triggered by changes, not only by a 30-day calendar.",
    "Why it does not belong in the plugin-seam gate: in the architecture doc, the plugin seam (`PlatformCapability`, H1 row of §5 at flui-global-architecture.md:304; W6 track B at :633) covers clipboard, haptics and dialogs. It never touches IME. The Windows TSF spike and implementation sit in the P slot of W1-W2, and 'Windows live + Narrator' sits in W3 under milestone B1 (flui-global-architecture.md:627-636). A plugin gate that waits on IME evidence would put the seam's API work behind TSF + Parley text-store (W4). The plugin-author concern that is real is the contract: can a third-party text widget implement the text store? That is testable headless.",
    "Win32 backend has NO IME path today. `grep -rln 'WM_IME|ImmGetContext|ITextStore|ITfThreadMgr|Imm32|ImmSet' crates/` returned nothing. crates/flui-platform/Cargo.toml:89-105 enables `windows` features without Win32_UI_Input_Ime or Win32_UI_TextServices. `grep -rn 'fn text_input' crates/flui-platform/src/platforms/` finds overrides only in headless/platform.rs:1250, macos/window.rs:944 and winit/window.rs:313; the Win32 backend inherits the default (None). Text reaches it only as WM_CHAR bursts (platforms/windows/events.rs:38-41, 323-331). A Japanese-IME gate therefore cannot be met until the TSF implementation from D10 lands (flui-global-architecture.md:602, 'Push-only IME' problem П12 at :65).",
    "Existing Windows evidence: `cargo xtask device windows-a11y` (tools/xtask/src/device/windows_a11y.rs:1-18). A UIA client finds the probe window, requires named texts and the button, invokes the button via IUIAutomationInvokePattern and reads the count back. It exits 0/1/2, with 2 = CANNOT_VERIFY. `cargo xtask device windows-input` (tools/xtask/src/device/windows_input.rs:1-23) sends real SendInput clicks, Tab and Enter, requires the probe to be the foreground window, and restores the cursor. On non-Windows hosts both print a skip that says they need 'Windows with an interactive desktop' (tools/xtask/src/device.rs:379-384). The device module is 1682 lines total (`wc -l tools/xtask/src/device/*.rs`).",
    "docs/BETA.md:122 records Windows (Win32) as **experimental**, not beta. Evidence dated 2026-09-24: one host, a manual run, 'the UIA client is not a Narrator session', 'no text entry, IME, resize, DPI change or lifecycle verification'. The Windows row of the BETA matrix therefore already records exactly the gap this question is about.",
    "docs/BETA.md:97-108 already requires each platform record to carry commit, OS, toolchain, backend, command, result and artifact. 'Reuse evidence only when its applicability to the candidate is explained' is effectively a freshness rule that no gate enforces. plan.md:60 has the metric 'платформы со свежим свидетельством (1 → 4 → 6)' ('platforms with fresh evidence') but defines no freshness window. The architecture doc (P5, flui-global-architecture.md:84) proposes structured `docs/evidence/*.toml` rendered into BETA.md, which is the natural place for a `commit` + `paths` staleness check in xtask.",
    "CI today: the `platform-windows` job (.github/workflows/ci.yml:1049-1080) runs only `cargo nextest run -p flui-platform` on windows-latest, both default and all-features. That includes winit tests that drive a real event loop, so real windows are created on the hosted runner. `gpu-test` also runs on windows-latest (ci.yml:953). No job runs `cargo xtask device windows-*`.",
    "desktop-mcp cannot drive an IME as written. tools/desktop-mcp/src/input/device.rs:538-570 `type_text` sends every character on Windows through `crate::os::send_unicode`, i.e. the KEYEVENTF_UNICODE/VK_PACKET path. The input method does not see that path (general Win32 behavior; hypothesis, not tested here). Driving the Japanese IME would need virtual-key romaji strokes with the IME opened (ImmSetOpenStatus or the TSF open-close compartment), plus Space/Enter for conversion. The composition would be read back through UIA TextPattern once the backend exposes it (D10 plans TSF plus UIA TextPattern/ValuePattern, flui-global-architecture.md:284).",
    "Maintainer host state (experiment below): Windows 11 Pro. Enabled input languages are only en-US (0409) and ru (0419). The Japanese IME binaries exist (C:\\Windows\\System32\\IME\\IMEJP) but ja-JP is not in the user language list, and Narrator.exe exists. A live Japanese session needs a one-time settings change by the owner. I did not make that change: it is a system setting.",
    "The architecture doc's open question 8 (flui-global-architecture.md, §14, last item) is exactly this proposal. The migration table already puts `cargo xtask device windows-input` live evidence as acceptance for D1b Win32 (flui-global-architecture.md detail table, 'D1b (по бэкенду)' ('per backend')). Live Windows evidence is therefore already a per-step acceptance there, not a horizon gate."
  ],
  "market_precedents": [
    {
      "who": "boringcactus (Rust GUI survey 2025)",
      "what": "Tested 43 Rust GUI crates on Windows with three checks: a label and text field with two-way binding, Windows Narrator reading it, and the Windows Japanese IME typing 東京 via 'toukyou' (both composition and the final conversion).",
      "outcome_or_lesson": "Only Dioxus, Slint and Tauri passed all three; WinSafe also passed but needs manual Win32 layout. GPUI, Iced and Floem failed Narrator; Iced and Floem failed IME. The author treats accessibility and IME as baseline requirements. This is the external bar FLUI's exit B1 copies: it is a public, one-shot, manual reviewer test, not a CI gate, so a dated manual record that is fresh at release time matches how FLUI will actually be judged.",
      "source": "https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html"
    },
    {
      "who": "AccessKit",
      "what": "The accesskit_windows tests create a real HWND (CreateWindowExW), call SetForegroundWindow, create a CUIAutomation8 client and wait for UIA focus events. The `cargo test (Windows)` job runs them on GitHub-hosted windows-latest.",
      "outcome_or_lesson": "A UIA client check against a real window runs on the hosted Windows runner. FLUI's windows-a11y check has the same shape (no synthesized input), so moving it into CI is plausible without a self-hosted runner. Hypothesis until run: SendInput plus the foreground requirement in windows-input may still get CANNOT_VERIFY there.",
      "source": "gh api repos/AccessKit/accesskit: adapters/windows/src/tests/mod.rs (lines ~148-253) and .github/workflows/ci.yml test job (windows-latest, `cargo test -p accesskit_windows`)"
    },
    {
      "who": "NV Access (NVDA)",
      "what": "NVDA's system tests (Robot Framework) start a real NVDA process with a sandbox profile, a speech-spy synth driver and a global plugin, send key presses, and assert on the captured speech. They run in NVDA's AppVeyor CI.",
      "outcome_or_lesson": "A real screen-reader session can be automated in CI by capturing its speech, but with NVDA, not Narrator, which has no speech-capture API. For FLUI, an NVDA speech-spy check is the path to automating a 'screen reader session'. A Narrator session stays a human record. These tests have also had recurring intermittent failures (e.g. nvda issue #15104, PR #20756), so expect flakiness.",
      "source": "https://github.com/nvaccess/nvda/tree/master/tests/system ; https://github.com/nvaccess/nvda/pull/20756"
    },
    {
      "who": "Chromium",
      "what": "The Windows TSF text store (ui/base/ime/win/tsf_text_store.cc) is covered by tsf_text_store_unittest.cc and mock_tsf_bridge, which exercise ITextStoreACP operations without a real IME installed.",
      "outcome_or_lesson": "Browsers put the contract gate at mock-TSF unit tests in CI and leave real-IME behavior to manual QA. This supports making the D10 text-store/TSF contract an automated H0 gate while the live Japanese IME session stays a B1 or release record.",
      "source": "https://chromium.googlesource.com/chromium/src/+/6373124a3638f6c9f77c7914f78b226163973c68/ui/base/ime/win/tsf_text_store.h ; https://issues.chromium.org/issues/40489775"
    },
    {
      "who": "Flutter (Windows embedder)",
      "what": "Flutter's Windows IME support is IMM32-based (TextInputManager, later renamed TextInputManagerWin32), not TSF. Open issues include external dictation tools not detecting Flutter TextFields (#182876) and IME composition leaking into the next TextField (#191196).",
      "outcome_or_lesson": "Even Flutter ships Windows IME without a live-IME release gate, and its bugs surface from users. The dictation issue shows that IMM32-only input misses TSF clients, which supports FLUI's D10 choice of TSF. Flutter is not a precedent for gating on live IME.",
      "source": "https://github.com/flutter/engine/pull/23853 ; https://github.com/flutter/flutter/issues/182876 ; https://github.com/flutter/flutter/issues/191196"
    },
    {
      "who": "GitHub Actions hosted Windows runners (community reports)",
      "what": "Reports say hosted Windows runners have a fixed 1024x768 display and may lack a proper interactive session for desktop UI tests; guidance for FlaUI/WinAppDriver-style tests is a self-hosted runner with auto-logon. `Install-Language ja-JP` was reported unavailable without admin on hosted runners; DISM capability installs are the alternative.",
      "outcome_or_lesson": "The evidence conflicts: AccessKit shows UIA plus real windows work on windows-latest. A Japanese IME on a hosted runner is unproven and would need language capability installation on every run. The IME lane should target the maintainer's 32-core host, or a Hyper-V VM or self-hosted runner on it, not windows-latest.",
      "source": "https://github.com/actions/runner-images/issues/2935 ; https://sep.com/blog/executing-windows-desktop-app-system-tests-in-ci/ ; https://github.com/orgs/community/discussions/68929"
    }
  ],
  "constraints": [
    "The gate cannot be met today: Win32 has no IME implementation (no text_input() override, no Ime/TextServices features). An H0 plugin gate on it would put the seam behind TSF (W2) and the Parley text-store (W4).",
    "Narrator has no programmatic speech-output API, so a 'Narrator session' can only be recorded by a human. An automatable screen-reader session means NVDA plus a speech spy (NVDA system-test precedent), which is another tool dependency for one maintainer.",
    "The maintainer host has no Japanese IME enabled (languages: en-US, ru). Adding it is a one-time OS settings change the owner must make. Agents should not make it (system settings).",
    "The desktop-mcp type_text path (KEYEVENTF_UNICODE) bypasses IMEs (general Win32 behavior; hypothesis, not tested here), so desktop-mcp cannot automate IME without new virtual-key/IME-open support.",
    "windows-latest: UIA against a real window is likely fine (AccessKit precedent). SendInput/foreground (windows-input) and a Japanese IME on a hosted runner are unproven; installing the ja-JP language capability per run is slow and needs admin (hypothesis).",
    "AGENTS.md: a new gate must be both a `cargo xtask` command and a step in a CI job that the `ci` aggregator gates. A live human-session record cannot be a merge gate. It can only be a release or status gate: BETA.md platform status, or a release-check that verifies the freshness of docs/evidence/*.toml.",
    "Cost for one maintainer: a calendar-based freshness rule (<30 days) forces about 12 manual Windows sessions a year even when nothing Windows-related changed. A rule triggered by path changes plus 'fresh at every release tag' costs about the number of Windows-touching releases. Estimate, not measured.",
    "CI minutes: gpu-test (60 min timeout) and platform-windows (45 min) already run on windows-latest in the heavy lane. A windows-a11y CI step would add a release build of examples/a11y_probe (not measured)."
  ],
  "experiments_run": [
    "`grep -rln \"WM_IME\\|ImmGetContext\\|ITextStore\\|ITfThreadMgr\\|Imm32\\|ImmSet\" crates/` returned nothing. `grep -rn \"fn text_input\" crates/flui-platform/src/platforms/` found only headless/platform.rs:1250, macos/window.rs:944 and winit/window.rs:313, which shows Win32 has no IME path.",
    "`sed -n 80,120p crates/flui-platform/Cargo.toml` showed the Win32 feature list without Win32_UI_Input_Ime or Win32_UI_TextServices.",
    "`grep -n windows-latest .github/workflows/ci.yml` found platform-windows (line 1053, flui-platform nextest only) and gpu-test (line 953). No `cargo xtask device windows-*` step exists in CI.",
    "PowerShell `Get-WinUserLanguageList` returned 'en-US :: 0409:00000409' and 'ru :: 0419:00000419' (no ja-JP). `Test-Path C:\\Windows\\System32\\Narrator.exe` returned True. C:\\Windows\\System32\\IME lists IMEJP, IMEKR, IMETC and SHARED.",
    "`sed -n 525,570p tools/desktop-mcp/src/input/device.rs` showed that type_text on Windows uses crate::os::send_unicode per character.",
    "`gh api` on AccessKit/accesskit: adapters/windows/src/tests/mod.rs uses CreateWindowExW, SetForegroundWindow and CoCreateInstance(CUIAutomation8). Its ci.yml `test` job runs `cargo test -p accesskit_windows` on windows-latest.",
    "`git log --format=\"%ad %s\" --date=short -- docs/BETA.md` shows the Windows a11y and input evidence landed 2026-09-23 (#1285, #1286). desktop-mcp's last commit is 2026-09-24 (d6ac4f936).",
    "Web: fetched the boringcactus 2025 survey (method and results), plus searches on the NVDA system tests, the Chromium TSF mock tests, the Flutter Windows IMM32 embedder, and hosted-runner interactive-session and language-pack reports.",
    "Not run (to respect the shared memory-limited host and read-only scope): `cargo xtask device windows-a11y` and `windows-input` locally, and any windows-latest CI trial. Whether windows-a11y and windows-input pass on a hosted runner is a hypothesis, to be tested with a throwaway workflow on a branch."
  ]
}
```

## options

```json
{
  "options": [
    {
      "id": "A",
      "name": "Hard H0 gate: live Japanese IME (TSF) + Narrator session recorded before the plugin seam / H0 exit",
      "description": "The app/plugin-author judge's proposal as written. H0 cannot exit, and the PlatformCapability plugin seam cannot be declared stable, until BETA.md holds a human-recorded Windows session: typing 東京 via 'toukyou' in the Japanese IME (both composition and conversion) and Narrator reading the field and the label.",
      "pros": [
        "It matches the exact public bar FLUI will be judged by. boringcactus 2025 tested 43 crates, and only Dioxus, Slint and Tauri passed label + Narrator + IME (https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html).",
        "It forces TSF work to start early. Push-only IME (problem П12, flui-global-architecture.md:65) is the main known Windows hole."
      ],
      "cons": [
        "It cannot be met today. Win32 has no IME path at all: a grep for WM_IME/ITextStore/ITfThreadMgr/Imm32 in crates/ found nothing, there is no text_input() override in platforms/windows, and crates/flui-platform/Cargo.toml:89-105 enables neither Win32_UI_Input_Ime nor Win32_UI_TextServices. Text reaches the backend only as WM_CHAR (platforms/windows/events.rs:38-41, 323-331).",
        "It couples unrelated work. The plugin seam (§5 H1 row, flui-global-architecture.md:304; W6 track B at :633) covers clipboard, haptics and dialogs, and never touches IME. The gate would put the seam behind TSF (W1-W2) and the Parley text-store (W4).",
        "A human session is not a merge gate under AGENTS.md (a gate must be an xtask command plus a CI step). It can only be a status or release gate, so as an 'H0 gate' nothing enforces it.",
        "The host has no ja-JP enabled (Get-WinUserLanguageList: en-US, ru). The owner has to change OS settings once.",
        "Narrator has no speech-capture API, so the check is manual forever."
      ],
      "cost_now": "High. It blocks the H0 exit on implementing TSF (weeks), plus a manual session and an OS language setup.",
      "cost_later": "Low extra cost once met, but every change to the Windows text path needs another manual re-record with no rule for when.",
      "reversibility": "Easy to drop on paper, but the schedule slip it causes (seam waiting on TSF) cannot be undone.",
      "fits_plan": "Poor. It contradicts the W-order in flui-global-architecture.md:627-636, where TSF sits in W1-W2 (P slot) and 'Windows live + Narrator' sits in W3 under B1. It also conflicts with plan.md, which treats evidence as a per-platform freshness metric (plan.md:60), not a horizon gate."
    },
    {
      "id": "B",
      "name": "Status quo: the live session stays only in exit B1, no other change",
      "description": "Keep the live Japanese IME + Narrator record as a B1 exit criterion only. The Windows row in BETA.md stays 'experimental' (docs/BETA.md:122) until the record exists. No new CI lanes and no freshness rule.",
      "pros": [
        "Zero cost now.",
        "Consistent with the plan's milestone ordering. Windows is honestly marked experimental and already lists 'no text entry, IME ... verification' as the gap (docs/BETA.md:122)."
      ],
      "cons": [
        "No contract gate. A third-party text widget could be built against a text-store trait that turns out to be unimplementable over TSF, and the problem would surface only at B1.",
        "The one-time B1 record goes stale silently. docs/BETA.md:97-108 asks that reuse of evidence be justified, but nothing enforces it, and plan.md:60's 'fresh evidence' metric has no definition.",
        "The existing automation (cargo xtask device windows-a11y / windows-input, tools/xtask/src/device/*.rs) keeps running only by hand. No CI job calls it (grep of ci.yml: only platform-windows and gpu-test run on windows-latest), so it can regress unnoticed."
      ],
      "cost_now": "None.",
      "cost_later": "Medium-high. Regressions in the Win32 a11y/input path go unnoticed until the manual re-run, and B1 validity at release time is argued by hand.",
      "reversibility": "Fully reversible, since nothing is built.",
      "fits_plan": "Matches the milestone text but leaves the P5 structured-evidence idea (docs/evidence/*.toml, flui-global-architecture.md:84) and the freshness metric unimplemented."
    },
    {
      "id": "C",
      "name": "Calendar freshness: Windows beta status requires a live record less than 30 days old",
      "description": "Keep the live session in B1. After B1, Windows keeps 'beta' in BETA.md only while the latest human Japanese IME + Narrator record is under 30 days old. An xtask release-check reads docs/evidence/windows.toml and fails the release when the record is stale.",
      "pros": [
        "Simple, mechanically checkable rule. An xtask check reads a date.",
        "It keeps the public claim honest over time."
      ],
      "cons": [
        "About 12 manual Windows sessions a year for one maintainer, even in months when nothing Windows-related changed (estimate).",
        "It measures the calendar, not risk. A risky TSF refactor merged on day 2 after a record passes as 'fresh' for 28 days.",
        "Being manual, it gets skipped in practice. The likely outcome is status flapping between beta and experimental, which is worse for users than a stable honest label."
      ],
      "cost_now": "Low-medium: an evidence TOML schema plus an xtask date check.",
      "cost_later": "High and recurring maintainer time with poor signal.",
      "reversibility": "Easy: change the window or drop the rule.",
      "fits_plan": "Partially. It gives plan.md:60's 'fresh evidence' a definition, but the wrong one for a single-maintainer project."
    },
    {
      "id": "D",
      "name": "Hybrid, split by what is automatable: headless contract gate in H0 + automated Windows lane + human session in B1 with change-triggered freshness",
      "description": "Four parts. (1) H0 contract gate, in CI: the D10 pull text-store trait ships with a headless conformance kit (composition ranges, selection, replace-range, surrogate and grapheme edges) that any text widget, including a plugin's, must pass. Once the TSF backend exists, it also gets unit tests against a mock ITextStoreACP sink or thread manager, the way Chromium tests tsf_text_store with mock_tsf_bridge. (2) Automated Windows lane: first a throwaway branch workflow checks whether `cargo xtask device windows-a11y` passes on windows-latest (AccessKit runs a UIA client against a real HWND there). If it passes, add it as a step in the heavy-lane platform-windows job (ci.yml:1049-1080), mapping exit 2 (CANNOT_VERIFY) to a failure, not a skip, on that runner. windows-input, and a new `cargo xtask device windows-ime` (virtual-key romaji plus opening the IME, read back through UIA TextPattern), run on the maintainer host or a Hyper-V VM on it, not on windows-latest. (3) The human Japanese IME + Narrator session stays an exit-B1 criterion, recorded in docs/evidence/windows.toml (commit, OS, IME version, command, result, artifact) and rendered into BETA.md (P5). (4) Freshness is triggered by changes plus releases, not the calendar. A release-check xtask marks the Windows beta record stale when paths under crates/flui-platform/src/platforms/windows/**, the text-input trait or the semantics bridge changed since the recorded commit. It also requires a record at the current release tag's line of development. The optional NVDA speech-spy automation stays deferred until a real regression shows it is needed.",
      "pros": [
        "Each concern goes to the cheapest layer that can actually fail. The plugin author's real risk, whether a third-party widget can implement the text store, becomes a headless test that runs on every PR and does not wait for TSF.",
        "It reuses what already exists (windows-a11y/windows-input in tools/xtask/src/device, desktop-mcp) instead of building new tooling up front.",
        "Re-records happen only when Windows text or a11y code changes, or at a release. That is far fewer sessions than 30-day windows for one maintainer, and each one is tied to real risk.",
        "The public bar (boringcactus: label + Narrator + IME) is still met by a dated human record at release time, which is how reviewers will judge FLUI.",
        "It satisfies AGENTS.md: parts 1-2 are xtask commands plus CI steps; parts 3-4 are a release/status gate, not a pretend merge gate."
      ],
      "cons": [
        "More moving parts: a conformance kit, an evidence TOML plus a staleness check, and possibly a CI step.",
        "Hosted-runner behavior is unproven (hypothesis). windows-a11y may get CANNOT_VERIFY on windows-latest, which would move even the UIA lane to the maintainer host.",
        "windows-ime automation needs new desktop-mcp/xtask input work. The type_text in tools/desktop-mcp/src/input/device.rs:538-570 sends KEYEVENTF_UNICODE, which likely bypasses the IME (hypothesis).",
        "Path-based staleness can miss indirect breakage (for example a Parley change that alters caret ranges). Mitigate this by including the text-store crate paths in the trigger set and by the release-tag requirement."
      ],
      "cost_now": "Low-medium. Spec the conformance-kit shape with D10 now (no TSF needed), run one throwaway windows-latest workflow trial, and define the evidence TOML schema. The staleness xtask is roughly a day of work (estimate).",
      "cost_later": "Medium, but bounded: TSF mock tests when the backend lands (W1-W2), a windows-ime xtask before B1, the owner enabling ja-JP once, and one manual session per Windows-touching release.",
      "reversibility": "High. Each part is independent. The path trigger set can be widened or narrowed, a CI step can be demoted to a manual lane, and a calendar ceiling (for example 90 days) can be added later if records drift.",
      "fits_plan": "Good. H0 gets a contract gate that does not reorder W1-W8. TSF stays in W1-W2, live Windows plus Narrator stays in W3/B1 (flui-global-architecture.md:627-636), P5 structured evidence becomes the mechanism, and plan.md:60's 'fresh evidence' metric gets an enforceable definition. It answers §14 open question 8."
    }
  ],
  "recommended": "D",
  "rationale": "Don't make the live Japanese IME + Narrator session an H0 gate for the plugin seam. Four reasons:\n\n1. It cannot be met today. The Win32 backend has no IME path: no WM_IME, IMM32 or TSF code, no text_input() override, and no Ime/TextServices features in crates/flui-platform/Cargo.toml:89-105.\n2. The seam doesn't touch IME. It covers clipboard, haptics and dialogs (flui-global-architecture.md:304, :633). Gating it on IME would put the seam behind TSF (W1-W2) and the Parley text-store (W4).\n3. A human session can't be a merge gate under AGENTS.md.\n4. Narrator has no speech-capture API, so that half can never be automated.\n\nWhat the plugin author really worries about is the contract: can a third-party text widget implement the pull text-store? That can be tested headless now. Chromium does the same: it checks its TSF text store in CI with mocks and leaves real-IME behavior to manual QA.\n\nSo in H0 the conformance kit becomes the gate. We also add an automated Windows lane that reuses the existing `cargo xtask device windows-a11y`. Whether it works on windows-latest is still a hypothesis (AccessKit's UIA tests do run there), so a throwaway branch run checks that first. The IME and SendInput checks run on the maintainer host or a VM on it.\n\nThe human session stays in exit B1, which is the public bar boringcactus 2025 sets. The record is kept in docs/evidence/windows.toml. It goes stale when the Windows platform, text-input or semantics paths change after the recorded commit, and a current record is needed for every release. That costs one maintainer far less than a 30-day calendar rule (about 12 manual sessions a year) and matches re-recording to real risk. If records turn out to drift anyway, a longer calendar limit can be added later.\n\nTwo actions only the owner can take: enable ja-JP once in Windows settings (the host has only en-US and ru), and approve the throwaway windows-latest workflow trial. Not verified here: whether windows-a11y or windows-input pass on a hosted runner, and whether the desktop-mcp KEYEVENTF_UNICODE path really bypasses the IME."
}
```

## judge_q8_windows_evidence_gate_engineer

```json
{
  "choice": "D",
  "confidence": 0.82,
  "reasons": [
    "Option A cannot be satisfied at H0. The crates contain no IME code: a Grep for WM_IME|ITextStore|ITfThreadMgr|ImmGetContext over D:\\flui\\crates returned 'No files found', and Win32 has no text_input() override. Gating the plugin seam on IME would make PlatformCapability (clipboard, haptics, dialogs) wait for TSF (W1-W2) and the Parley text-store (W4). It would reorder the migration with nothing gained for the seam.",
    "AGENTS.md says a gate is an xtask command plus a CI step under the ci aggregator. A human session with Japanese IME and Narrator cannot meet that, and Narrator offers no speech capture. So an 'H0 gate' built on it would not be enforced. At most it could be a status or release check.",
    "The plugin author's real risk is whether a third-party text widget can implement the pull text-store. That can be tested headless now as a conformance kit, and Chromium's mock-TSF unit tests show the pattern. It belongs at H0 and costs nothing on Windows.",
    "B leaves the existing automation unused. No CI job runs xtask device (ci.yml has windows-latest only at :953 gpu-test and :1053 platform-windows). The single-run B1 record would also go stale with nothing noticing.",
    "C bases freshness on the calendar rather than on what changed. That means about 12 manual sessions a year for one maintainer, and the Windows status would keep flipping. D ties re-recording to changes in the Windows, text-input or semantics paths and to each release tag. The cost scales with actual risk, and a calendar ceiling can still be added later.",
    "D fits the existing structure: docs/BETA.md:97-108 already asks for a structured evidence record per platform (commit, OS, command, artifact), and the Windows row is honestly 'experimental'. The boringcactus bar (label + Narrator + IME) is a one-time manual review done from outside the project, which a dated B1 record that is fresh at release time answers exactly."
  ],
  "conditions": [
    "Close §14 open question 8 in the architecture doc explicitly. H0 exit requires the text-store conformance kit to be green in CI. The live Japanese IME + Narrator record is a B1 and release criterion, not an H0 criterion.",
    "Treat the windows-latest lane as a hypothesis until a throwaway branch workflow shows it working. Get owner approval before any .github/workflows change, since AGENTS.md says to leave workflows alone unless the task is about them. On that runner, exit code 2 (CANNOT_VERIFY) must fail the step, never skip it silently. If it cannot pass reliably, record the lane as maintainer-host-only in BETA.md rather than adding retries.",
    "The staleness check must be a real cargo xtask command wired into a release-check step. Its trigger set must cover platforms/windows/**, the text-input trait, the semantics/UIA bridge and the text-store crate paths. The trigger list lives in one place next to docs/evidence/windows.toml, and a test must fail when a listed path changes after the recorded commit.",
    "The conformance kit must be the same kit the in-tree TextField uses, and it must include surrogate, grapheme and composition-range edge cases. A kit that only exercises a mock implementation would pass whether or not the real contract holds.",
    "The Windows status cannot become 'beta' until a human-recorded session exists at the release commit, with the IME typing 東京 via 'toukyou' through composition and conversion, and Narrator reading both the label and the field. UIA-client automation does not count as the Narrator half.",
    "Only the owner may enable ja-JP on the host. Agents must not change OS language settings. The windows-ime automation must first prove that it goes through the IME (virtual-key romaji with the IME open), not KEYEVENTF_UNICODE, before it counts as evidence.",
    "Add a calendar ceiling (for example 90 days) only if records turn out to drift in practice. Do not start with one."
  ]
}
```

## judge_q8_windows_evidence_gate_ecosystem_author

```json
{
  "choice": "D",
  "confidence": 0.82,
  "reasons": [
    "The A gate cannot be met today. A grep for `fn text_input|WM_IME|ITextStore|ImmGetContext` in crates/flui-platform/src/platforms matched only winit/window.rs:313, headless/platform.rs:1250 and macos/window.rs:944. Win32 has no IME path, so an H0 gate on a live Japanese IME puts the plugin seam behind TSF (W1-W2) and the Parley text-store (W4). A plugin author would wait on unrelated work.",
    "The risk that actually matters to a third-party text-widget or package author is contract stability, not whether Windows passes live today. The question is whether a widget written outside the repo can implement the pull text-store and survive TSF landing. A headless conformance kit exercises exactly that on every PR, and it can ship in H0. Chromium tests its TSF store with mocks the same way.",
    "B leaves that contract risk hidden until B1. It also leaves the existing windows-a11y and windows-input xtask commands manual-only, so they can regress without anyone noticing. docs/BETA.md:97-108 asks that reused evidence explain why it still applies, but nothing enforces this.",
    "C's 30-day rule tracks the calendar, not the risk. It means about 12 manual Windows sessions a year for one maintainer, and the Windows status would likely flap between beta and experimental. For app authors that is worse than a stable, honest label.",
    "D keeps the public bar that boringcactus 2025 uses (label + Narrator + IME) as a dated human record in B1. It fits AGENTS.md: human sessions become a release/status gate, and only automatable checks become merge gates. It is also reversible part by part.",
    "Narrator has no speech-capture API, so that half stays manual whatever we choose. D is the only option that is honest about this."
  ],
  "conditions": [
    "The text-store conformance kit is a public, versioned test-support API that crates outside the repo can run against their own widgets. It must not be an internal-only test module; otherwise the plugin-author benefit disappears. It covers composition ranges, selection, replace-range, UTF-16 offsets (TSF uses ACP/UTF-16), surrogates and graphemes.",
    "The text-store trait shape from D10 is written against TSF ITextStoreACP semantics (lock model, UTF-16 ACP offsets, async edit sessions) before H0 freezes it. Otherwise the gate can pass while the contract is still unimplementable on Windows.",
    "The windows-latest step is added only after a throwaway branch run shows `cargo xtask device windows-a11y` passing there. In CI, exit 2 (CANNOT_VERIFY) counts as a failure, not a skip. If the run fails, the lane moves to the maintainer host or a VM, and BETA.md says so.",
    "Freshness is triggered by path changes, and the trigger set must include the Win32 platform dir, the text-input/text-store trait crate paths, Parley/text-editing paths and the semantics/UIA bridge. It also requires a current record for every release tag. Add a calendar ceiling of about 90 days only if records drift.",
    "Until the human Japanese IME + Narrator record exists, the BETA.md Windows row stays 'experimental' and explicitly lists 'no IME'. No README or marketing text may claim Windows IME or accessibility parity before then.",
    "Only the owner enables ja-JP in Windows settings. Agents do not change system settings. The windows-ime xtask must use virtual-key romaji and open the IME, not the KEYEVENTF_UNICODE path in desktop-mcp. Whether that path bypasses the IME is still a hypothesis to test."
  ]
}
```

## judge_q8_windows_evidence_gate_owner

```json
{
  "choice": "D",
  "confidence": 0.8,
  "reasons": [
    "Option A's gate cannot be met today. I re-checked: grep for WM_IME|ITextStore|ITfThreadMgr|ImmGetContext in D:\\flui\\crates returns 0 matches. An H0 gate tied to TSF would stall the plugin seam behind W1-W2 TSF and W4 Parley text-store work. The seam (clipboard, haptics, dialogs; flui-global-architecture.md:304, :633) has no IME dependency.",
    "The plugin author's real risk is whether a third-party widget can implement the pull text-store. A headless conformance kit that runs on every PR tests that directly. It follows Chromium, which covers its TSF text store with mock-TSF unit tests and leaves real-IME behavior to manual QA.",
    "Under AGENTS.md a human session can never be a merge gate; a gate must be an xtask command plus a CI step. So the live Japanese IME + Narrator record belongs in the release/status layer: exit B1 and the Windows row of BETA.md, which is 'experimental' today (docs/BETA.md:122).",
    "For one maintainer, re-recording when relevant paths change or a release is cut costs far less than a 30-day calendar rule (about 12 sessions a year), and each re-record is tied to real risk. That gives plan.md:60's 'fresh evidence' metric a definition someone can maintain.",
    "It reuses tools that already exist (tools/xtask/src/device/windows_a11y.rs, windows_input.rs, desktop-mcp) and every part can be reversed separately. Option B leaves those checks running only by hand and lets the B1 record go stale silently."
  ],
  "conditions": [
    "WIP limit: stage it. In H0, deliver only (1) the conformance kit spec and headless kit together with the D10 trait, and (2) one throwaway windows-latest trial of windows-a11y, only after the owner approves it. Do the evidence TOML and staleness check with P5, not earlier. Do the windows-ime xtask and the TSF mock tests only when the TSF backend lands (W1-W2 to W3). Never have more than one of these parts in progress at a time.",
    "If windows-a11y gets CANNOT_VERIFY (exit 2) on windows-latest, do not add it as a skippable step. Keep it on the maintainer host and record that decision. If it does go into CI, exit 2 on that runner counts as a failure.",
    "The staleness trigger paths must cover crates/flui-platform/src/platforms/windows/**, the text-input/text-store trait, the semantics bridge and the text-layout crates behind caret ranges. A current record is also required at every release tag, whatever the paths say.",
    "Do not add NVDA speech-spy automation or a calendar ceiling up front. Add a long ceiling (about 90 days) only if records are shown to drift.",
    "The owner makes the OS change that enables ja-JP; agents do not touch system settings. Until then, the Windows row stays 'experimental', with IME listed as the gap.",
    "Before any windows-ime automation relies on it, verify that desktop-mcp's KEYEVENTF_UNICODE path bypasses the IME. It is unverified today.",
    "Record the answer to §14 open question 8 as a decision, so the H0 exit criteria name the conformance kit, not the live session."
  ]
}
```

## verify

```json
{
  "holds": true,
  "problems": [
    {
      "problem": "The conformance kit in part (1) tests a contract the architecture itself calls read-only and synchronous. TSF's ITextStoreACP also needs writes (SetText, InsertTextAtSelection) and an asynchronous lock grant (RequestLock, then OnLockGranted, with TS_S_ASYNC). A headless kit written against the §4.8 surface could go green while the trait still cannot be implemented over TSF, and that is the exact risk D says the kit removes.",
      "evidence": "flui-global-architecture.md:284 describes the surface as \"синхронная read-only поверхность text-store (текст в диапазоне, выделение, composing, rect для диапазона, индекс по точке) на owner-потоке\", which has no write verbs. Option D's kit lists \"replace-range\", a write. What is in the tree today is push-only: crates/flui-interaction/src/text_input.rs:285-305 (TextInputHandle attach, detach, set_cursor_area; events dispatched at :194). The TSF write and async-lock requirements come from Microsoft's ITextStoreACP documentation; I did not re-fetch it in this run, so treat that part as a hypothesis.",
      "severity": "major",
      "fix": "Before H0 freezes the kit, write D10 as a read + edit + lock contract: UTF-16 ACP offsets, write verbs, and a lock or edit-session model that can be granted later. Make the kit a TSF-shaped harness that requests locks asynchronously and edits from the IME side, not just a set of range queries. Record this as an amendment to §4.8 (:284), otherwise the kit can pass both ways."
    },
    {
      "problem": "D says exit 2 (CANNOT_VERIFY) would be mapped to a failure on the runner, but that mapping already happens, so it adds nothing. The real problem runs the other way. windows-a11y reports CANNOT_VERIFY only when COM or UIA cannot be created. A runner with no interactive desktop or foreground would therefore show FAIL (exit 1, 'window not found' or 'count did not advance') or a timeout, which reads as a product regression rather than a host limitation.",
      "evidence": "tools/xtask/src/device/uia.rs:55-63: the only paths to Start::CannotVerify are the Com::init and CoCreateInstance(CUIAutomation) failures. tools/xtask/src/device/plan.rs:249-256: execute returns ExitCode::from(code), so any non-zero code already fails a GitHub step. windows_a11y.rs:52-54: when no window with the button is found, it returns Ok(false), which is FAIL.",
      "severity": "minor",
      "fix": "For the throwaway windows-latest trial, add a pre-check that separates host limits from regressions: the probe window is found and visible, and a desktop session exists (for example GetForegroundWindow or OpenInputDesktop succeeding). Report that pre-check as CANNOT_VERIFY before counting a FAIL. Drop the 'map exit 2 to failure' wording."
    },
    {
      "problem": "Adding windows-a11y to the platform-windows job adds a release build of the whole facade (material, a11y, wgpu stack) to a job that today compiles only flui-platform. That changes the job's cost and cache profile, and D does not account for it.",
      "evidence": "tools/xtask/src/device.rs:479-490: build_a11y_probe runs `cargo build -p flui --locked --release --example a11y_probe --features material,a11y`. .github/workflows/ci.yml:1049-1080: platform-windows runs only `cargo nextest run -p flui-platform` (default and all-features, 45-min timeout). gpu-test (ci.yml:953) already builds the wgpu stack on windows-latest, though in debug with WARP.",
      "severity": "minor",
      "fix": "If the trial passes, put the step in gpu-test, which already carries the wgpu build and has WARP, or in a separate heavy job listed in the ci aggregator's needs and HEAVY_JOBS. Or build the probe in debug. Measure the job-time change during the trial."
    },
    {
      "problem": "The path-based staleness check in part (4) needs git history back to the recorded commit. The release workflow checks out shallow, so a release-time `git diff <recorded>..HEAD -- <paths>` would error or give a wrong answer unless the workflow is changed. D doesn't list that change, and AGENTS.md treats workflow edits as owner-approved scope.",
      "evidence": ".github/workflows/release.yml:19-21 triggers on tags v*, and :52-54 uses actions/checkout with only persist-credentials: false (default fetch-depth 1). The only fetch-depth: 0 in the workflows is ci.yml:314.",
      "severity": "minor",
      "fix": "Run the staleness check locally as part of `cargo xtask release-check` before tagging, where full history exists. Or list 'fetch-depth: 0 in release.yml' explicitly as an owner-approved workflow change. Have the xtask exit with a clear error, not a pass, when the recorded commit is not in local history."
    },
    {
      "problem": "The engineer judge's condition that a human session exist 'at the release commit' is circular. The evidence TOML that records the session is committed after the commit it tested, so the release commit can never equal the recorded commit.",
      "evidence": "Option D part (3) records the commit in docs/evidence/windows.toml, and the engineer condition says 'human-recorded session exists at the release commit'. Committing the TOML necessarily produces a later commit.",
      "severity": "minor",
      "fix": "Define freshness as: the recorded commit is an ancestor of the release commit, and no trigger path changed between the two. The evidence files themselves are excluded from the trigger set."
    }
  ]
}
```
