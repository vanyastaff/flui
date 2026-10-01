# CI baseline, 2026-10-01

Evidence: `gh run view <id> --json databaseId,status,conclusion,createdAt,updatedAt,jobs,headSha,event,displayTitle,headBranch` and `gh run view <id> --log`. Raw job/step timestamps and cache log excerpts are in [ci-baseline-2026-10.json](ci-baseline-2026-10.json). Times below use UTC timestamps, not displayed local times.

Wall time is creation to final job completion, including initial queue and dependency waits. Runner minutes sum startedAt-to-completedAt for non-skipped completed jobs; they are elapsed occupancy, not invoiced OS-weighted minutes. The final substantive job is the latest finishing job excluding ci; it is not necessarily the longest-running job.

| Run | Event | Wall min | Runner min | Initial queue min | Final substantive job | Rust-cache interpretation |
|---|---|---:|---:|---:|---|---|
| [36864134553](https://github.com/vanyastaff/flui/actions/runs/36864134553) | pull_request | 49.57 | 305.95 | 0.05 | test-windows | 12 misses / 2 total restores |
| [36894848200](https://github.com/vanyastaff/flui/actions/runs/36894848200) | pull_request | 51.42 | 316.20 | 0.08 | test-windows | 12 misses / 2 total restores |
| [36901416035](https://github.com/vanyastaff/flui/actions/runs/36901416035) | push | 30.73 | 223.83 | 0.07 | test-nested | 8 misses / 4 total restores |
| [36847748985](https://github.com/vanyastaff/flui/actions/runs/36847748985) | push | 54.52 | 161.02 | 32.12 | feature-matrix (3/3) | 0 misses / 12 total restores |
| [36847273915](https://github.com/vanyastaff/flui/actions/runs/36847273915) | schedule | 36.50 | 225.58 | 0.05 | test-windows | 2 misses / 12 total restores |
| [36844419342](https://github.com/vanyastaff/flui/actions/runs/36844419342) | pull_request | 30.58 | 220.95 | 2.17 | test-windows | 2 misses / 12 total restores |
| [36815991864](https://github.com/vanyastaff/flui/actions/runs/36815991864) | push | 22.05 | 148.27 | 0.05 | feature-matrix (2/3) | 0 misses / 12 total restores |
| [36814487195](https://github.com/vanyastaff/flui/actions/runs/36814487195) | pull_request | 18.97 | 65.78 | 0.07 | test-nested | 0 misses / 11 total restores |
| [36813995888](https://github.com/vanyastaff/flui/actions/runs/36813995888) | push | 17.98 | 144.23 | 0.07 | feature-matrix (2/3) | 0 misses / 12 total restores |
| [36812515217](https://github.com/vanyastaff/flui/actions/runs/36812515217) | pull_request | 18.95 | 69.98 | 0.05 | test-nested | 0 misses / 11 total restores |
| [36788150403](https://github.com/vanyastaff/flui/actions/runs/36788150403) | pull_request | 17.92 | 68.75 | 0.03 | test-nested | 4 misses / 5 total restores |

The restore count includes actionlint and lychee caches. Run 36864134553 is confirmed cold for every Rust cache (12 misses, two tool restores). Runs 36847748985, 36815991864, 36814487195, 36813995888 and 36812515217 have no Rust misses. These are observed cold/warm runs, not a controlled same-commit experiment. Changes to feature graphs and different lane selections prevent interpreting the duration difference as cache-only savings.

Ordinary PR runs have 13 active jobs; the expanded PR and scheduled runs have 24. Main has 22 active jobs. Across the three ordinary PR samples, wall time is 17.92–18.97 min and occupancy 65.78–69.98 min. Across the four completed expanded/scheduled samples, wall time is 30.58–51.42 min and occupancy 220.95–316.20 min. Main sample wall time is 17.98–54.52 min and occupancy 144.23–161.02 min. Main run 36847748985 spends 32.12 min before its first job starts; its 54.52 min wall time must not be attributed to compilation.

## Cold expanded run step timings

| Job | Step | Seconds |
|---|---|---:|
| gpu-test | Run actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 | 15 |
| gpu-test | Install Rust stable | 21 |
| gpu-test | Cache cargo registry + target | 58 |
| gpu-test | cargo xtask gpu-test (readback suites, WARP) | 1457 |
| macos-ci | Run actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 | 5 |
| macos-ci | Install Rust stable + clippy + rustfmt (+ iOS target) | 10 |
| macos-ci | Add the iOS target to the pinned toolchain | 16 |
| macos-ci | cargo xtask ci | 2237 |
| macos-ci | cargo clippy (ios runner) | 62 |
| macos-ci | Complete job | 6 |
| feature-matrix (3/3) | Install Rust stable + clippy | 13 |
| feature-matrix (3/3) | cargo xtask feature-matrix --slice 3/3 | 1170 |
| feature-matrix (2/3) | Install Rust stable + clippy | 10 |
| feature-matrix (2/3) | cargo xtask feature-matrix --slice 2/3 | 1130 |
| test-windows | Run actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 | 10 |
| test-windows | Install Rust stable | 24 |
| test-windows | Cache cargo registry + target | 58 |
| test-windows | cargo xtask test --no-trybuild | 2552 |
| feature-matrix (combinations) | Install Rust stable + clippy | 10 |
| feature-matrix (combinations) | cargo xtask feature-matrix --slice combinations | 505 |
| feature-matrix (1/3) | Install Rust stable + clippy | 11 |
| feature-matrix (1/3) | cargo xtask feature-matrix --slice 1/3 | 583 |

Log-level Cargo Finished and nextest Summary messages confirm that most time is build/link and external-consumer compilation. Cargo Finished combines compilation/linking; logs do not expose CPU/RAM pressure or an independent link-time total. Nested projects report time inside their parent nextest test, so treating it as ordinary test execution would conceal compilation. Cache restore setup is visible per step; PR save-if false means no upload benefit or upload cost in these samples.

## Limits and open observations

- Run 36894848200 completed successfully: 51.42 wall minutes and 316.20 runner minutes. Windows took 46.27 min, macOS 40.90 min, GPU 24.92 min. The refreshed snapshot includes final job timestamps and cache/build/test log excerpts. Both expanded PR runs are cold for every Rust cache; the later commit is not a controlled repeat of the first.
- Cache restore success is not proof Cargo reused all artifacts. Cargo fingerprints, feature changes and pruning can still cause rebuilds.
- The logs do not contain enough resource telemetry to establish contention, Cargo lock delay, disk-pressure cause or eviction history. Those remain hypotheses unless additional log evidence is collected.
- There is no controlled after experiment here. The changes must be measured separately on comparable cold/warm commits.

Cargo logs reproduce the supplied cold-run observations exactly: Windows first build 25m17s, additional build 2m49s, 596 tests 98.785s, nine nested tests 514.734s. macOS test build 11m06s, 596 tests 39.152s, 14 nested tests 735.460s. GPU builds 10m37s and 7m09s; 56 tests 136.297s and the external consumer test 3.474s. These excerpts are preserved in the raw JSON. The Windows suite reports one leaky test; that diagnostic is existing baseline behavior and must not be suppressed by the optimization.

## Observed restore and cache post-step costs

These are action step elapsed times from completed main runs, not pure transfer benchmarks. Restore includes action fingerprint/setup work; post includes pruning and save/upload work when a save occurs. A zero or one-second post step generally does not upload an entry. Distinguishing transfer from compression requires detailed action logs.

| Main run | GPU restore sec | GPU post sec | Workspace restores sec | Workspace post sec | Xtask restore sec | Xtask post sec |
|---|---:|---:|---|---|---|---|
| 36847748985 | 246 | 60 | 32, 28, 32, 27, 22 | 1, 17, 23, 12, 12 | 16, 23, 18, 14 | 7, 0, 0, 0 |
| 36815991864 | 88 | 1 | 27, 28, 48, 19, 20 | 0, 0, 0, 0, 0 | 15, 13, 13, 16 | 0, 0, 0, 0 |
| 36813995888 | 60 | 26 | 27, 41, 28, 26, 20 | 12, 19, 1, 13, 11 | 15, 17, 17, 14 | 6, 0, 0, 0 |

## Feature matrix balance

| Job | Min duration min | Max duration min | Samples |
|---|---:|---:|---:|
| feature-matrix (1/3) | 6.42 | 10.47 | 7 |
| feature-matrix (2/3) | 14.57 | 20.70 | 7 |
| feature-matrix (3/3) | 14.15 | 20.62 | 7 |
| feature-matrix (combinations) | 7.03 | 9.33 | 7 |

These ranges span different commits and feature graphs. They establish observed imbalance, not its cause. Both cold expanded PRs finish shard 1/3 earlier than shards 2/3 and 3/3; feature-matrix never controls their final green time because Windows finishes later. Removing or rearranging feature cases cannot be justified solely by these totals.


## Latest main comparator

Main run [36901416035](https://github.com/vanyastaff/flui/actions/runs/36901416035), commit `e8909cfca`, completed successfully after merging the final external-texture change. Its tree is identical to PR head `33c22f6213fd93769313f1e07e65cce1b71bdb68`: `git diff --name-only 33c22f6213fd93769313f1e07e65cce1b71bdb68 e8909cfca` produces no output. This removes source-tree differences from this comparator, but the main lane has 22 active jobs instead of the expanded PR's 24, so the workflow totals remain incomparable as cache-only effects.

The latest main has 30.73 min wall time and 223.83 runner minutes, with initial queue 0.07 min. Its critical job is test-nested (26.82 min), followed by GPU (21.18), feature shard 2/3 (19.75), shard 3/3 (19.33), test (16.38), and test-features (15.30). Eight Rust restore steps miss; deps and the final ci job restore the xtask entry after earlier main jobs have saved it. Two tool caches also hit. This is largely a cold main observation, not a warm comparator.

The GPU restore step takes 86s despite a miss (action setup/pruning); its GPU test command takes 1108s and cache post step 37s. Test and nested restores take 13s each; test-fast is 808s, build-all-targets 103s, nested command 1574s. Test post step takes 13s and nested post step 1s. Raw step timestamps and cache evidence are appended to the JSON. Adding this sample expands observed main occupancy to 144.23–223.83 min; the wall range remains 17.98–54.52 min.
