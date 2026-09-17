QUESTION: What did the 2026-09-13 A-D Widget/Element/Inherited audit prove, and are its candidates already filed?

ANSWER: The Hegel report is a substantive A-D pass, not an exhaustive widget-catalog proof. It verifies selected widget/state/build/dependency behavior with 191 passing selected tests and source/reference review, rejects several suspected defects as contract-compatible, and leaves two executable coverage gaps. Its two known P2 candidates are already represented by open issues #1072 and #1073.

VERSIONS: flui at b482a20824ba80c961a5c5074e2df6a8880f8b1e for the Hegel report; Flutter reference tag 3.44.0 at 559ffa3f75e7402d65a8def9c28389a9b2e6fe42.

SOURCES:
- /mnt/data/audits/view-ad-20260913/REPORT.md:1 records the A-D audit scope.
- /mnt/data/audits/view-ad-20260913/REPORT.md:3 states no new filing candidate was established by that pass and names the two prior P2 candidates for parent inspection.
- /mnt/data/audits/view-ad-20260913/REPORT.md:21 documents covered widget/state API behavior and current provider-level dependency semantics.
- /mnt/data/audits/view-ad-20260913/REPORT.md:29 documents the build mutation/order source path and selected coverage.
- /mnt/data/audits/view-ad-20260913/REPORT.md:43 documents dependency recording/lifecycle coverage and the conservative retention disposition.
- /mnt/data/audits/view-ad-20260913/REPORT.md:47 records the unsatisfied inherited lookup reparenting candidate.
- /mnt/data/audits/view-ad-20260913/REPORT.md:55 records provider-level invalidation and ADR0008 field-mask status.
- /mnt/data/audits/view-ad-20260913/REPORT.md:59 records the pending equal-Memo plus inherited invalidation executable gap.
- /mnt/data/audits/view-ad-20260913/REPORT.md:83 records the selected test commands and the 191-test passed total.
- https://github.com/vanyastaff/flui/issues/1072 is the open GlobalKey concrete-type replacement issue matching the report's first P2 candidate.
- https://github.com/vanyastaff/flui/issues/1073 is the open unsatisfied inherited dependency reparenting issue matching the report's second P2 candidate.

OPEN: Conditional-read disappearance/retention and equal-Memo plus independently pending inherited invalidation remain coverage gaps, not filed defects. The report itself is not a full A-D proof or full workspace gate.

ANSWERED
