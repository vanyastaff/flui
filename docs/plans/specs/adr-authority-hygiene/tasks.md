# ADR hygiene implementation tasks

**Status:** Approved by the maintainer on 2026-10-11. Implementation, clause/reference migration, independent reviews and available local validation completed on 2026-10-12. PR publication and CI acceptance remain pending.

**Requirements:** https://github.com/vanyastaff/flui/issues/1553.

The task graph allows factual corrections and registry navigation after inventory refresh. Font absorption requires its complete clause/reference mapping before deletion. Validation and independent review depend on every edit and migrated reference.

- [x] Refresh the pinned 160-record inventory only for intervening changes. Classify current acceptance, implemented portion, native execution evidence, supersession scope and actual remaining obligations.
- [x] Apply factual prose/status/reference corrections in one reviewable group. Reconcile ADR-0155 through scoped ADR-0200; clarify ADR-0069's editing authority without changing native code; leave ADR-0135's poison policy for its separate decision. Supporting source comments passed their actual source-scope checks.
- [x] Keep ADR-0125/0179/0191 as independent identity, controller-lifetime and topology decisions. Build one current-authority navigation map in existing architecture documentation; verify every live clause, exception, cited test and inherited scope.
- [x] Correct scoped registry backlinks and shorten only proven duplicated prose. Add dated factual amendments; a normative reversal requires explicit new supersession. Preserve identity/exhaustion/owner/single-parent rules without source behavior change or retirement of these three records.
- [x] Map ADR-0016/0059/0077 to Accepted ADR-0092 and immutable text-spike evidence. Migrate source/design/current-spec/architecture references, preserve archive meaning, then delete approved obsolete records and dead exemptions.
- [x] Keep ADR-0102. Leave ADR-0057/0099 conditional until ADR-0102/0103 acceptance is evidenced or separately approved. Keep all 19 Proposed records and incomplete ADR-0142 obligations visible.
- [x] Add current-authority navigation for motion, notification/retention and text/native families through existing architecture documents. Do not merge unrelated boundaries into one large ADR.
- [x] Run existing documentation/source-scope checks and staged reference search. Every old-ID occurrence is classified and every local target resolves; every migrated architectural rule still has its producer/test authority.
- [x] Review semantics separately from link hygiene. Accurate authority, not fewer files, is completion. Independent standards and spec reviews passed after the clip and Vsync comment corrections.
- [ ] Publish the approved documentation PR separately from presentation behavior changes and confirm CI acceptance.

No new behavior test is required for pure prose. A discovered source behavior change returns to its own approved spec and public RED first.

## Local validation evidence

Base: `6af2cf426809dbf8a33bbb415e904f0bfdc847c4`. Implementation source: `ed96d3396e49352a8512c34bc880209641efdaed`; subsequent unused-exemption removal: `556c0b52e68fe188d857aaf5618ed1ebf44a6722`. The final status update changes only these approved archival spec files.

`cargo xtask check-changed --base 6af2cf426809dbf8a33bbb415e904f0bfdc847c4` classified 13 changed packages and 21 dependents. Fmt and both scoped clippy passes passed. The Windows driver then could not replace its own running `xtask.exe`; this original invocation failed. Its exact unfinished plan from `--dry-run` was continued directly, rather than treating the source comments as docs-only or claiming a green driver invocation.

- The computed nextest scope passed 889 unit/integration tests: 875 in the restricted run and all 14 failed/timeout fixture cases in a targeted serial retry outside the sandbox. Initial failures denied temporary Git/Cargo paths, DLL publication and localhost sockets. The 29 configured skips remain excluded from execution claims.
- Strict rustdoc and the full computed doctest scope passed. Doctests required an outside-sandbox retry because rustdoc could not write temporary executables; configured ignored examples remain ignored.
- Required Windows native-feature clippy, scoped wasm clippy and the no-default-features wasm hot-reload facade check passed. Both per-feature clippy passes completed all 106 invocations each, including test/bench/example compilation.
- `trybuild_ui::ui_tests` passed in flui-platform's `compiler_guards` integration binary, one test with no skips. This checks compiler contracts; the native platform runtime suite still requires Linux/xvfb.
- Every repository-check subcommand has passing evidence. The first `cargo xtask checks` run found two stale exemptions after the approved cleanup. Removing only those unused entries made targeted docs-paths and markers pass; unchanged successful subchecks were retained. Documentation links reported zero errors across 2117 links. The initial mixed run remains recorded as failed.

macOS source checks require unavailable `cargo-zigbuild`, iOS requires a genuine Apple SDK on macOS, and Android requires an NDK. These configured omissions remain CI responsibilities. Optional Windows and wasm paths were typechecked; this package adds no physical input or cross-platform execution claim.
