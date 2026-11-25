# FLUI CLI architecture

`flui-cli` owns command parsing, project template selection, generation of
consumer dependency declarations, platform scaffolding and build
orchestration (`src/build/`), and the dev-loop source watcher
(`src/watch.rs`). It links no framework runtime crate (its FLUI dependencies
are the contract crate `flui-protocol` and `flui-driver`'s trace writer): the
runtime half of hot reload lives in `flui-hot-reload`, which the *app* links,
and the only contract between the two is a pair of environment-variable names
pinned by a dev-dependency test.

## One output policy, no logging framework

Everything the CLI prints goes through `ui::`. Human narration is styled
text on stderr; `--json` is NDJSON on stdout; warnings and errors survive
`--quiet`; diagnostics (the commands run, the probes made, the paths skipped)
are `ui::debug` lines shown under `-v` only. There is no `tracing`
subscriber and no `RUST_LOG`: with no framework runtime crate in the graph, a log
filter would have nothing to select but the CLI's own lines, and a second
voice on stderr would compete with the narration. `flui-log` is the
application's logging backend, not the CLI's.

## Mapping decisions

### Android runs build for the selected device's native ABI

The run session reads the selected device's ordered ABI list before invoking
cargo-ndk. It chooses the first supported ABI instead of forcing ARM64 on x86
emulators. Environment validation checks that ABI's exact Rust target in the
project's toolchain (`rustc --print target-libdir --target <triple>` under a
deadline, so a toolchain without rustup is read too). A target without
libraries still builds when the project's cargo configuration (or
`CARGO_UNSTABLE_BUILD_STD`) asks for `build-std` and the toolchain has
`rust-src` (`a_target_is_buildable_from_its_libraries_or_from_source`);
otherwise the refusal names the toolchain looked at (release and sysroot), its
`rustup target add` command and the `build-std` alternative, before cargo-ndk
runs. The tool installs nothing itself.

The `targetSdk` is FLUI's declared value from `platform-support.toml`, never
lowered to fit the machine: it names the behaviour changes FLUI meets. The
packager without Gradle links against the highest installed `android.jar`,
which must not be below it, and checks that, with the build-tools, before the
native build; a missing platform is `BuildError::AndroidPlatformMissing`, naming
the levels installed and the one needed (`a_platform_older_than_the_target_is_refused`
in `the_apk_meets_the_store_levels`). A failed build with hot reload disabled exits as a build failure;
there is no watcher that could repair it. `android_run_builds_the_selected_device_native_abi`
pins the target argument, missing-target admission and bounded failure through the CLI process.

Both SDK-tools APK packaging and generated Gradle projects take the Android
floor from `platform-support.toml` (`ANDROID_MIN_SDK` in
`build/platform_floor.rs`), the level the application's safe-area capture
needs. `the_link_declares_the_floor_and_the_pinned_target` pins the aapt2
invocation;
`scaffolded_platforms_ignore_and_clean_what_builds_write` checks the generated
application configuration through the CLI.

The Android package comes from `flui.toml`'s `[app] id` on every build, whichever packages the
APK. The SDK-tools path writes it into the manifest it links; for a Gradle project `flui build`
writes it into `gradle.properties` as `flui.applicationId` beside `flui.minSdk` and
`flui.targetSdk`, and the scaffolded `app/build.gradle.kts` reads it. A changed identifier
therefore reaches the APK that `flui run` then starts and copies scene plugins into by that same
package. A literal `minSdk` or `applicationId` in the script that differs, or one the build cannot
read, is a second statement and is refused before Gradle runs
(`a_build_script_states_what_flui_toml_states_or_is_refused`). The `namespace` stays as scaffolded:
it is the package of the app's own Java or Kotlin code.

### One version, one NDK and one signing key reach both packagers

`flui.toml`'s `[app] version` is SemVer, parsed when the file loads (`build/app_version.rs`), and
is the only source of every package's version: Android's `versionName` is the version as written
and its `versionCode` is `major * 10000 + minor * 100 + patch`, refused when a minor or patch
component is above 99, when the code is above Google Play's 2 100 000 000, or for `0.0.0`; Apple's
bundle versions take `major.minor.patch`. The packager without Gradle passes both to `aapt2 link`;
a Gradle project receives `flui.versionCode` and `flui.versionName`, and a literal in its script
that differs is refused (`gradle_states_the_version_the_ndk_and_the_release_key_once`).

The NDK `flui build` resolves (`ANDROID_NDK_HOME`, else the highest version under the SDK's `ndk/`,
compared by number) is exported as `ANDROID_NDK_HOME` and `ANDROID_HOME` on every `cargo ndk`
call, the scene plugin's included, and passed to Gradle as `-Pflui.ndkPath`, which the scaffolded
script reads as its `ndkPath`; an `ndkVersion` or another `ndkPath` in the script is refused. A
relative `ANDROID_HOME` or `ANDROID_NDK_HOME` is made absolute once, against the directory the
tool started in, so `cargo ndk` and Gradle get the same directory.

Invariant: a build never writes this machine's or this build's values into the committed
`gradle.properties` of the project's Android platform. That file holds the project's facts (floor, target,
package, version), the same on every machine; the NDK, the Java sources and R8 rules of the linked
crates (paths into this machine's cargo registry) and the release key go on the `gradlew` command
line as `-Pflui.…`, and the scaffolded script reads each with a fallback, so a plain `./gradlew`
still builds, with Gradle's own NDK and the debug key. A build leaves the scaffolded tree byte for
byte as it was (`a_build_leaves_the_scaffolded_project_as_it_was`), and removes those properties
from a file an older `flui build` wrote them into.

Which key signs is decided once (`build/android_signing.rs`): a debug build signs with the debug
keystore where the Android Gradle plugin keeps it (`ANDROID_USER_HOME`, `ANDROID_PREFS_ROOT` or
`ANDROID_SDK_HOME`, then Java's `user.home`: `USERPROFILE` on Windows, `HOME` elsewhere), so an APK
from either packager installs over the other's; a release build signs with `[signing.android]`
(keystore and alias), its passwords read from `FLUI_ANDROID_KEYSTORE_PASSWORD` and
`FLUI_ANDROID_KEY_PASSWORD` by `apksigner` (`env:`) and by Gradle, never written to argv or a file;
a release of an application that names no key is signed with the debug key and the build warns,
so `flui run --release` still installs. A Gradle release whose script reads no
`flui.signing.storeFile` is refused before Gradle runs, and an `app-<profile>-unsigned.apk` newer
than the signed one is refused after (`a_gradle_apk_that_is_not_signed_is_refused`). Both packagers
write `<name>-<profile>.apk`, and the packager without Gradle removes its unsigned intermediates.
It also states `android:extractNativeLibs="false"`, which the Android Gradle plugin writes itself,
so the stored, aligned libraries are mapped from the archive
(`the_linked_manifest_maps_the_libraries_from_the_archive`).

`flui run --scene` pushes to one device by serial (`--device`, or the one online) and copies the
plugin with `adb -s <serial> shell run-as <AppId> cp <staged> files/<lib>`, each word its own
argument, since `adb shell` joins them for the device's shell; the package is an `AppId`, and the
library is the one `cargo ndk -o` copied for the scene crate
(`the_scene_plugin_is_copied_into_the_application_on_the_named_device`).

### Manifests declare what the linked crates declare

A crate states what it needs from the application's manifests in
`[package.metadata.flui.declares]` (ADR-0307): a `schema` version, then per-platform keys, and
under `features.<feature>` what a feature adds while it is enabled. `declarations.rs` joins the
tables of the crates the built artifact links, each with the features the build enables for it.

Invariant: a manifest declares nothing for code the artifact does not link. `cargo metadata`
resolves one graph for everything a workspace can build, so its features count what a
dev-dependency, a build script's dependency or a procedural macro's own copy of a crate turns on.
Every collection therefore takes the crates and their features from `declarations/linked.rs`,
which reads `cargo tree` over normal edges without procedural macros for the target, and takes
only the tables and the edges from `cargo metadata --filter-platform`
(`declarations_are_joined_over_the_graph_the_target_links`).

One entry, `Declarations::collect(Build, targets)`, serves the Android, iOS, macOS and Linux
writers, the one-instance check and `flui doctor`. A `Build` is a directory, a `Selection` and
the features the build turns on. The selection is what the build compiles: a named member, an
example of one (which links its package's dev-dependencies), or cargo's default for the
directory, which is the package there and at a workspace's root its default member, whether the
root is virtual or a package itself (`the_default_selection_is_what_cargo_builds_there`). A build compiles one
application: the default selection at a root with several default members fails naming them,
because one application's manifests do not join another's declarations, and the package built
stays the application when a crate of its build depends on it
(`only_the_application_declares_the_links_it_opens`). A packager reads the member its build
compiled (`BuildArtifacts::package`), not the member's siblings
(`a_desktop_entry_registers_what_the_built_member_opens`,
`an_ios_bundle_reads_the_declarations_of_the_built_member`). Android builds `cargo ndk rustc
-p <package> --lib --crate-type cdylib`, naming the package whose manifest is in the project's
directory, since `cargo rustc` compiles exactly one target.
`flui run` selects the binary plain `cargo run` would run, as `flui build` selects a desktop
binary (a root package's default members; among several default members, the one `default-run`
when exactly one names one: `a_run_without_a_package_selects_the_binary_cargo_runs`), and names
it to every cargo command of the run, so the check reads the build that runs: plain `cargo run`
among several default members would turn on the features every member asks of a shared
dependency. The features the run builds with come from one function that gives both the cargo
commands and the check (`a_desktop_run_checks_what_it_builds`).

A crate is its `PackageId` from end to end. Cargo is asked for the compiled package by id, so a
dependency that carries the application's name is not mistaken for it
(`a_dependency_may_share_the_applications_name`). The listing is read with its depths: each line
resolves among the dependencies of the crate it is listed under, by name and version, and where
that crate depends on two packages of one name and version, by the source the package's id names
(a git reference encoded as cargo prints it, a sparse index with its `sparse+`). The depth cargo
prints ends at a `|` the format starts with, since a package name may start with a digit. A
registry cargo lists by its configured name is read back to its index as cargo reads it
(`CARGO_REGISTRIES_<NAME>_INDEX`, then the nearest cargo configuration file, the one without the
extension beside a `.cargo/config.toml` when both are there, up to the one in `$CARGO_HOME`, a
relative `CARGO_HOME` read against the directory cargo runs in); a name none of those configures stands for any registry's crate there
(`registries_are_read_from_the_nearest_configuration`). A line that names no package, or a listing that leaves out the compiled package, fails the
build instead of producing a smaller manifest (`a_listing_reads_back_as_each_crates_features`,
`a_fork_on_an_encoded_branch_keeps_its_features`).

`flui add` prints what the added crate declares for each platform whose manifests the build
writes, from that platform's build on any host: the Android permissions of the Android build (a
feature only it turns on included) and the Apple accessed APIs of the iOS build, never what a
feature only another platform's build or the application's tests turn on. It finds the crate by
the application's dependency edge, or the facade's, under the key the application names it by,
in each target's own graph, so a crate linked at one version for Android and another for iOS is
read at each (`add_links_a_capability_and_shows_its_registration`,
`add_reads_each_platforms_version_of_the_crate`).

The table is strict: an unknown key, a schema newer than the CLI reads, a feature the crate lacks
or a malformed value fails the build naming the crate
(`a_malformed_declaration_is_refused_by_crate`). The CLI holds nobody's needs as literals:
`flui-native` declares `VIBRATE` for its haptic patterns, and `flui-app`'s `http` feature declares
`INTERNET` for `HttpURLConnection`, which an application gets only with the facade's `http` on
(`the_manifest_declares_what_the_linked_crates_need`).

Android's `AndroidManifest.xml` is generated whole on every build (ADR-0307 §4) at
`<project>/platforms/android/app/src/main/AndroidManifest.xml`, where both packagers read it: FLUI's base
(`templates/android/AndroidManifest.xml`), the joined permissions, the label from `flui.toml` and
the application's `cdylib`. It is written only when it changed. A file there without the
generated-file header is refused untouched rather than overwritten
(`the_manifest_is_rewritten_only_when_flui_wrote_it`).

The base names `FluiActivity` with `hasCode`, `singleTop` and the back flag
(`the_base_names_flui_activity_with_the_code_it_needs`), whose comment says what the host does: it
registers no back callback, so every back from Android 13 is the system's; a launch in another
application's task is forwarded to the application's own task (ADR-0366). Its class comes from the dex
`flui-native` declares (`android.dex`, schema 2), which the packager without Gradle stores in the
APK as `classes.dex` (then `classes2.dex`, … for further crates), and Gradle compiles the declared
sources (`android.java`) from `-Pflui.javaSources`, refusing a build script that does not read it
(`libraries_and_dex_files_are_appended_where_the_platform_reads_them`,
`gradle_compiles_the_java_sources_the_crates_declare`). No `javac` or `d8` runs on the SDK-tools
path. The library checks the classes' protocol before it starts (ADR-0255 decision 5). The R8
rules the crates declare (`android.proguard`; `flui-native` keeps `dev.flui.**`, which only native
code names) reach a Gradle release as `-Pflui.proguardFiles`, beside the app's own scaffolded
`proguard-rules.pro`, and a script that does not read them is refused
(`a_minified_release_keeps_what_the_crates_rules_keep`). The generated manifest and the
scaffolded script have goldens (`the_generated_manifest_matches_its_golden`,
`the_scaffolded_gradle_script_matches_its_golden`).

A content provider a crate ships (`android.providers`, schema 4: a class and an authority suffix,
checked in `declarations/provider.rs`) becomes a `<provider>` after `FluiActivity`, unexported and
granting URI permissions, under the authority `<application id>.<suffix>` (ADR-0377 §4): the
packager without Gradle writes `flui.toml`'s id, the package it links, and a Gradle build
`${applicationId}`, which Gradle replaces with the variant's id, an `applicationIdSuffix`
included; the provider serves under the authority Android attaches it with. One suffix with two
classes, or one class with two suffixes, is refused naming both crates
(`the_application_declares_the_providers_its_crates_ship`, `a_provider_authority_has_one_class`).

The iOS bundle's `PrivacyInfo.xcprivacy` lists the required-reason APIs the graph of the device or
simulator target declares (`apple.privacy.accessed-apis`), joined per category with the
application's own `<project>/platforms/ios/PrivacyInfo.xcprivacy`; tracking and collected data stay the
application's. Rust links statically, so no per-SDK privacy bundle exists and the application's
manifest carries the union. `flui-native` declares `systemUptime` (`35F9.1`) and file timestamps
(`C617.1`), `flui-assets` file timestamps (`publication_contract`).

What the application opens (schema 3: `url-schemes`, `associated-domains`, `file-types`, each
value checked in `declarations/opening.rs`) becomes `FluiActivity`'s intent filters, a verified
one for the domains' `https` links (`the_activity_filters_what_the_application_opens`), and the
keys both Apple bundles' `Info.plist` take from `build/apple_opening.rs`: `CFBundleURLTypes`, and
per file type a document type (ADR-0361). A type the system declares itself (`public.png`,
`com.adobe.pdf`: the table `APPLE_SYSTEM_TYPES` in `declarations/opening.rs`) is named by that
identifier and opened as an alternate viewer, never declared; a type the application crate
declares is exported as a uniform type under the bundle identifier that the bundle opens as its
owner; a type only a linked crate declares is imported under that identifier, opened as an
alternate viewer, and warned about (`a_bundle_registers_what_the_application_opens`,
`a_type_the_system_declares_is_opened_and_not_owned`). Joined across the targets of one build, as
across its crates, an extension keeps one MIME type
(`declarations_joined_across_targets_give_an_extension_one_type`). The macOS bundle's XML plist
is rendered whole from the same keys, every value escaped by the renderer
(`the_keys_render_as_xml_property_list_entries`); every name a generated XML file takes
(`[app] display_name`, a bundle name, a file type's name) is refused when it holds a character
XML cannot carry (`util::xml::is_xml_name_text`). Every package's version numbers come from one
rule, `AppVersion::package_numbers`: `CFBundleShortVersionString` is `major.minor.patch`, the
version as written is `versionName` and `FLUIVersion`, and the build number (`versionCode`,
`CFBundleVersion`) is `[app] build`, else derived, with a release build of a pre-release refused
without it; the derived code's limits are checked when `flui.toml` loads, so every packager takes
the same versions (`every_package_states_the_one_version`,
`a_release_of_a_pre_release_needs_a_build_number`,
`a_version_no_packager_takes_is_refused_when_read`). A bundle named from its directory takes the
version of the package the build compiled (`AppBundle::package_numbers`,
`the_macos_info_plist_states_the_application_version`). The macOS bundle decides its plist, and
makes every refusal, before it removes an earlier bundle. A library build for iOS is always
delivered as an XCFramework, whatever Xcode project the project's iOS directory holds
(`a_library_build_beside_an_xcode_project_is_delivered_as_a_framework`). `[app] name` is a
`ProjectName`, checked when `flui.toml` loads, so no generated Kotlin, XML, JSON or HTML file
receives a name that breaks out of its string; the shown name is `[app] display_name`
(`DisplayName`, free text that each writer escapes), used for `CFBundleDisplayName`, the desktop
entry's `Name`, the Android label and the web title. The file is refused past `MAX_CONFIG_BYTES`
before it is decoded (`a_project_file_is_checked_when_it_loads`). Schemes and domains are the
application's own: a crate another crate of the build links by a normal or build edge is refused
when it declares them (`only_the_application_declares_the_links_it_opens`), since the run admits
what `#[flui::main]` reads from the crate it marks. A Linux build writes the desktop entry and the
shared-mime-info package beside the executable (`build/linux_desktop.rs`,
`a_linux_desktop_registers_what_the_application_opens`). The entry is written for every Linux
build (ADR-0351), named by what the window carries and naming it again as `StartupWMClass`: the
id `flui.toml` declares, else the executable's name in ASCII (`app_id::executable_class`, held to
`flui_foundation::executable_class`), so two examples keep two entries
(`the_entry_is_named_by_what_the_window_carries`). No build passes `FLUI_APP_ID` itself
(`DesktopBuilder::cargo_env`, `a_build_compiles_no_application_id_in_by_itself`): the id reaches
the application only from the cargo configuration every command writes from `flui.toml`, so one
application is compiled with one identity whatever command, target or example builds it.

`flui doctor` reads the same declarations for a manifest the CLI does not write
(`declarations/verify.rs`, `commands/doctor/project.rs`): the project's hand-written Android
manifest and each `--manifest` path, an `AndroidManifest.xml` or an XML `PrivacyInfo.xcprivacy` or
`Info.plist`, named by its root element and, for a property list, by whether it names a
`CFBundleIdentifier`. An Android manifest is read for its permissions and the intent filters of
what the application opens (a `VIEW` filter's MIME types, a `BROWSABLE` one's schemes, a verified
one's `https` hosts), an `Info.plist` for its `CFBundleURLTypes` schemes and the extensions its
document types open (`doctor_lists_what_a_manifest_lacks_of_what_the_application_opens`).
Each missing entry is listed with the crates that declare it and the
line to add, under the stable ids `project.declarations`, `project.android-manifest` and
`project.manifest`. The merger is not run: an Android file that is not Gradle's merged manifest
carries a note saying so (`doctor_lists_what_a_manifest_lacks_of_the_crates_declarations`).

`flui doctor --domains` (`commands/doctor/domains.rs`) fetches each associated domain's
`apple-app-site-association` and `assetlinks.json` with `Https`: `ureq` over rustls with the
platform's verifier, the client a Linux host's HTTP capability runs (ADR-0280), `https` only, no
redirect, a 15-second deadline and 128 KiB. The fetch sits behind the `Fetch` trait, so
`each_associated_domain_is_checked_for_both_files` answers from a table without a network, and
`doctor_checks_the_associated_domains_only_when_asked` shows that nothing is fetched without
the flag.

`flui add <capability>` (`commands/add.rs`) links a capability crate: it enables the facade
feature that carries it (`toml_edit`, the file's formatting kept) or, without the facade, runs
`cargo add` from the FLUI checkout the app already depends on. It then reads the crate's
`[package.metadata.flui.capability] plugin` (`declarations/capability.rs`), the path of its
`Plugin + Default` type, and prints the registration line with what the crate declares. A change
whose crate states no plugin is undone with its lockfile
(`add_links_a_capability_and_shows_its_registration`).

### Local source dependencies are explicit Cargo paths

`flui create NAME --local` selects the current working directory as the source
checkout. `--local=PATH` selects another checkout. The path belongs to the source
framework; the independent `--path` option chooses the application's destination.
Using `=` for an explicit source preserves the unambiguous positional name in
`flui create --local NAME`.

All templates use the same validated dependency source. Resolve the source to
an absolute path and validate its required manifests before writing a project.
TOML serialization handles path escaping; paths that cannot be represented in a
UTF-8 Cargo manifest are errors rather than silently altered filenames.
Hot-reload host, worker, and types crates use that same source; their sibling
dependencies remain relative within the generated workspace.

The paths are absolute Cargo path dependencies, so the generated application
can sit anywhere; fixed `../../crates` paths coupled its location to the
framework checkout. A compiled-in developer path or implicit global SDK
discovery would make an installed CLI depend on hidden machine state, and an
explicit local path is predictable for developers and agents.

The tradeoff is deliberate: moving the generated application is supported,
while moving the source checkout requires updating the dependency paths.
Without `--local` the dependency is the git tag of the CLI's own release, until
the framework is on crates.io and the registry version replaces it
(`FRAMEWORK_ON_CRATES_IO` in `templates/source.rs`).
CLI integration tests cover external destinations and compile all three template
shapes; source-validation failures must leave no partially created application.

### Applications depend on the facade

Every application template declares only `flui` as a framework dependency.
Generated imports use its public prelude and modules. A local facade
dependency points at the checkout root; internal crate directories stay an
implementation detail. The root package and required source manifests are still
validated before output creation.

The hot-reload template adds the official package `flui-hot-reload` beside the
facade in each of its three crates: the facade names no official package
(ADR-0311 §6). Every crate takes its `app-plugin` feature. The host has two
features of its own: `linked`, the default, links the logic crate (built as
`cdylib` and `rlib`) and calls its build function directly, so a release build
loads no library and installs no hook; `reload` turns on the package's
`host-hook`, and the host then installs `WorkerReloadHook` with
`AppConfig::dev_reload`. `flui.toml` names `reload` as `[hot_reload]
reload_feature`, and `flui run` builds and runs the host with it and with its
default features off, so the running host never links the logic crate it
reloads. A local package dependency points at
`packages/flui-hot-reload` in the checkout, which `--local` validates. Sibling
app crates retain relative dependencies. The worker deliberately receives the
full facade dependency graph: this increases its compile graph but keeps
application authoring consistent with the host and ordinary projects. The
host/worker split is the reload itself, so the template has no build without
the package; an ordinary template never names it.
The generated worker uses the hygienic `hot_reload_worker!` macro.

Template integration tests assert exact framework dependency names, local root
paths and reload feature selection, then compile every shape outside the
repository.

### Default counter state and interaction

The counter template's root is `CounterView`, a `StatefulView` whose
`create_state` creates the count as a signal (`ctx.signal(0)`), so the state is
born bound to its element. The Increment button's `on_pressed` callback updates
the signal, which rebuilds the element that read it in `build`; `build` only
describes the current count, and there is no message type.

The generated tests (`GENERATED_COUNTER_TESTS` in `tests/cli_create.rs`) mount
the view with `flui::testing::Tester` and locate widgets with `find`:
`counter_increments` taps `find::button("Increment")` and checks the text from
0 to 1 to 2, `each_app_owns_its_state` checks that a second mount starts at 0,
and `counter_content_is_centred` measures the content on a 480 by 320 surface.
The external CLI regression runs `cargo test --lib` in the generated project and
requires each named test to pass and the passed count to equal that list. The
sole framework dependency remains `flui`; its `testing` feature is enabled only
in `[dev-dependencies]`, and a normal build's dependency graph excludes
`flui-testing`.


### Run admission uses Cargo package identity

`flui run` asks `cargo metadata --no-deps --format-version 1` for the current
manifest and selects that package by canonical manifest path. Cargo resolves
renamed dependencies and workspace inheritance; a normal declaration of `flui`
identifies an application. Legacy `flui-app` and `flui-widgets` declarations
remain accepted. Comments, similarly named packages, dev/build dependencies and
unrelated workspace members cannot admit a project. A virtual workspace root
needs an application package directory, except for the existing configured
worker hot-reload path. Metadata is declaration evidence; Cargo's subsequent
run still determines whether the selected application builds and executes.

Integration fixtures use real Cargo metadata and marker-printing binaries with
minimal local dependency identities. They verify admission and execution without
opening a window; they do not substitute for the generated application's own
interaction regression or live platform verification.

### `flui run --agent`, `flui mcp` and `flui record` share a session file and a contract crate

The commands meet at the app's driver agent (ADR-0239) through a small
credentials file, `target/flui-agent.json` (`CARGO_TARGET_DIR` honoured), and
the contract crate `flui-protocol`: the launch variables and the shortest token
(`launch`), the hello, the method names and each method's parameter type.

- **Launch.** `flui run --agent` is admitted on this desktop, on an Android
  device and in a browser with a remote protocol of its own. A debug
  build needs nothing of the application; a profile other than `dev` (whose
  debug assertions it cannot be sure of) builds with the `driver` feature of
  the `flui` facade, or of `flui-app` for an app that names the runtime crate,
  read from the same Cargo metadata that admits the project; a package with
  neither is refused. It creates a `Session`: a `0700` temp directory holding
  the socket (a pipe name on Windows), `MIN_TOKEN_LEN` random bytes as hex for
  the token, and the session file written `0600` and removed with the session
  unless a newer run replaced it. The same feature goes to `cargo build` and
  `cargo run`, or cargo would build twice. The token is in the app's
  environment and the file, never in the `--json` stream.
- **Android.** The app binds the abstract socket `@flui-agent-<hex>` (ADR-0288
  §6), which only the device reaches, so the session holds an
  `adb forward tcp:0 localabstract:<name>` and the file names that loopback
  port (`Endpoint::Forwarded`) where a desktop session names the app's own
  endpoint (`Endpoint::Local`). `am start` hands the app the endpoint and the
  token as the intent's string extras; `adb`'s connections arrive as the
  shell's user, which the app admits. The forward is removed with the session,
  after the app is stopped. A release build gets the `driver` feature through
  `cargo ndk`. The token stays out of every rendered `adb` command.
- **Browser.** A page has no socket, so the command reaches it for the client
  (ADR-0288 §6): it starts the browser itself (`agent::browser::launch`, a
  profile of its own, `--remote-debugging-port=0`, the WebSocket read off its
  stderr) and drives one tab through a `BrowserLink`, the five things the relay
  asks of a tab: install a preload and load a URL, call a function that gets a
  report function last and keep what it returns, call a function on what was
  kept, release what was kept, and read the tab's events. A command's answer
  that comes after the command gave up waiting is kept by the socket, and the
  link releases what it holds in the page. Each browser is spoken to in the protocol it
  has without a driver: WebDriver BiDi in Firefox (`bidi`, the report function
  is a channel), the DevTools protocol in the Chromium family (`cdp`, a
  binding); the session and the relay never see which. Both report a
  navigation once its document commits (`browsingContext.navigationCommitted`,
  `Page.frameNavigated`), so a Firefox older than `bidi::MIN_FIREFOX`, which
  reports no commit, is refused at `session.new`. The preload installs
  `PAGE_LAUNCH_GLOBAL` in top-level documents only, so every reload is given
  the launch again. The relay (`agent::browser::relay`) serves the session's
  usual local endpoint on one polling thread and carries each client to a
  connection of the export the launch names, its lines queued until the export
  appears. What the relay holds of each connection in the page is one state per
  client (`relay::Page`), and letting a client go does what that state says: a
  connection open in the page is closed there and released, and one the page
  ended is released once its client has been written what the page sent before
  the end, a reload included. A connect call the page did not answer in time
  closes its client at once and leaves an abandon owed to the page
  (`relay::Abandons`): every ID owed is abandoned in one call, which closes a
  connection the page opened late and refuses a call it has not run yet; it is
  tried a bounded number of times on a doubling backoff, dropped by a reload,
  and tried once more when the relay stops. A reload closes the clients whose
  connections went with the document, and closes those connections in the page
  all the same, in case one belongs to the next document. A line the page
  refuses leaves its connection ended when the page's end follows in the same
  events, and closes it otherwise; closing a connection the page already ended
  does nothing there. `flui mcp` does not change. The relay and
  `flui run`'s own endpoint (`agent::supervisor`, `run.reload`) poll their
  clients through `flui_protocol::endpoint::PolledStream` (`agent::listener::polled`),
  the one rule the app's agent and the driver read and write a local endpoint
  by on Windows as on Unix. The WebSocket is
  `tungstenite`'s handshake and framing over a loopback `TcpStream`, no TLS.
  Safari is refused ([The driver agent cannot reach a page in Safari](../../docs/gaps.md#the-driver-agent-cannot-reach-a-page-in-safari)).
- **Serve.** `flui mcp` speaks MCP over stdio (hand-written JSON-RPC: six
  tools, no SDK, so the CLI's graph does not grow) and forwards each call as
  one protocol method, its parameters checked against the method's parameter
  type before anything is sent. It connects on the first call and reconnects
  once per call, because the app restarts under `flui run`: a request that
  never reached the app goes again, one that did goes again only when its
  method reads, since one that acts may have run. A reply is awaited on the
  connection's own thread for a bounded time, so a peer that is not an agent
  cannot hang the MCP client. Element handles are scoped to a window, so a
  call without `window` is resolved only when the app has exactly one.
- **Record.** `flui record` (`agent::record`) connects in the `observer` role
  (ADR-0288 §8), so it records beside the driver `flui mcp` is, and asks
  `record.start` for the input layer (§11). The answer's header and every row
  of the `record` events then go to the file as the input recording's JSON
  Lines, each line flushed as it is written, so a recording cut off keeps
  what arrived. The connection hands events to whoever waits on it
  (`Connection::next_event`, `request_seeing`); a request alone passes them
  over. Ctrl-C (the flag `flui run` listens with) or the app's own `end` row
  ends the loop, and `record.stop` then answers what the recording came to,
  the rows admitted before the stop and the `end` row arriving ahead of the
  answer. A recording whose `end` row never came fails the command. With
  `--native <dir>` it asks for the native layer too, and each stream's
  `record_native` lines go to `flui_driver::trace::native::NativeFiles` (the writer a
  driver's trace shares): the rows to a part file
  as they arrive, flushed line by line, and at the end `<n>.jsonl`, its header
  naming every device the rows read (a stream's header names none, its
  devices being known only once its rows are), then `index.json`. The part
  files are written whichever way the recording ended, so a cut-off one keeps
  its native rows as well.
- **Tests.** `agent::mcp::tests` pin the tool-to-method mapping;
  `an_agent_reads_and_taps_the_app_run_with_agent_through_mcp` runs
  `flui run --agent`, `flui mcp` and a fixture app end to end, and
  `an_optimized_agent_build_turns_on_the_driver_feature_and_a_debug_one_needs_none`
  the feature rule (Unix; the Windows pipe is covered by `flui-app`'s agent
  tests and not yet by a CLI run).
  `mcp_reaches_an_android_app_through_its_forwarded_port` drives `flui mcp`
  over a forwarded port, and
  `an_android_session_forwards_a_port_to_a_fresh_abstract_socket_and_removes_it`
  pins the `adb` calls and the intent extras with an `adb` that logs them.
  `agent::browser::tests` run the relay against a tab of the test's own, and
  both dialects end to end against a real browser (a page standing in for the
  app, its launch, a connection, a reload): Chromium and Firefox where one is
  installed, which CI's Linux image has. `cli_record` runs `flui record`
  against a stand-in agent: a recording the app ends, one Ctrl-C stops, one cut
  off, one refused, and one holding the native layer, whose streams are
  written with the devices their rows read (Unix).

### Native iOS selection and launch

`build ios` means a native executable application; `--lib` opts into static-library
XCFramework delivery. Exact simulator UDIDs are resolved before Cargo. The booted
device's `SIMULATOR_ARCHS` must be one recognized architecture, consistent with
runtime supportedArchitectures when present. There is no host-run fallback.
Commands have deadlines and drain both output streams. Install and launch use
the validated bundle's identity and selected UDID, without shutting down devices.
Config SemVer maps to numeric Apple keys while the full value remains in FLUIVersion.
The unavailable-device integration test guards against accidental host execution;
SDK-required build-pipeline tests (`src/build/tests/`) verify actual application bundle delivery.

### One output policy, two audiences

Every command narrates through `src/ui.rs`, never by printing directly.
Human text goes to **stderr**; stdout is reserved for payloads a user would
pipe (completion scripts) and, under `--json`, for one NDJSON object per line
with an `event` discriminator (`doctor.check`, `device`, `run.app.log`, …).
`--quiet` drops narration but keeps warnings, errors and tool output. The policy
is a process-global `OnceLock` because a CLI has exactly one terminal.

Every listing and the dev loop emit JSON, not only a daemon, and JSON mode
never mixes human text into stdout.
The alternative — a `Context` parameter threaded through every helper — was
rejected as pure noise for commands that are free functions.

### Interactivity is decided once, up front

`ui::is_interactive()` requires a terminal on stdin and stderr and none of
`CI`, `FLUI_NON_INTERACTIVE`, `--non-interactive`, `--json`. Anything that
would prompt (the `create` wizard) or read hot-keys (`run`) consults it and
fails with `CliError::NonInteractive` (exit 7) and a hint instead of blocking.
A CI job that forgot the project name gets a one-line fix, not a hung runner.

### Exit codes are a contract

`CliError::exit_code` maps every variant to a documented table (0 success,
1 any other failure, 2 usage or unsupported target, 3 environment, 4
build/test/lint/format failure, 5 device not found, 6 not a FLUI project, 7
needs a terminal, 130 interrupted). Scripts branch on the code; the text is for
people.

### External tools run with a deadline

`src/proc.rs` bounds every environment probe. `flui devices` once hung forever
on macOS because `Safari -v` launches Safari rather than printing a version;
browser versions are now read from `Info.plist` and every probe is killed at
its deadline and reported as a timeout. A `Problem` row, not an error, is the
result of a missing or hung tool: discovery never fails the command. The
builders' own probes run through the same deadline (`PROBE_TIMEOUT`):
`rustc --print target-libdir` and `rustc -vV` in `build/util/environment.rs`,
`cargo ndk --version` in `build/android.rs`.

### The dev loop multiplexes four sources

`flui run` drives one `dev_loop` over a `ReloadStrategy` (process restart, a
browser session, an Android session, or the worker host for the host/worker
layout). The loop polls, in priority order: the Ctrl-C flag (set by a
`tokio::signal` listener on its own thread), hot-keys from the `KeyReader`
thread, which reads raw bytes from a Unix terminal (`r`/`R`/`c`/`h`/`q`; there
are none elsewhere), the child's exit status, and the debounced source watcher.
The child never shares stdin (it is null, and the key reader owns the
terminal), and in `--json` mode its stdout and stderr are piped and forwarded as
`run.app.log` so the machine stream stays pure. The child is
stopped on every exit path, including errors, so a failed `flui run` never
leaves an orphaned app; a real SIGINT with nobody at the keyboard exits 130.

## The build pipeline is a module, not a crate

`src/build/` has one consumer, this binary, and no framework dependency, so
it is a module rather than a crate: every builder, scaffold and cargo helper
in it exists because a command calls it. Its artifact-selection tests live
in `src/build/tests/` as unit modules; they build real Cargo fixtures and
re-execute the test binary as a worker.

### A web build writes its page; a project's own page is a named template

`flui build web` writes `index.html`, `flui.js` and `flui.css` on every build
(`build::web_page`, ADR-0255 decision 16). The page holds no inline script or
style, so it runs under `script-src 'self'; style-src 'self'`: it links the
stylesheet and loads `flui.js`, which imports and starts the module
`wasm-bindgen` wrote. It names `[web] base_href` as its `<base href>`, a
checked `BaseHref` (absolute, plain segments, no escape: it cannot name
another site), states `[web] description` and `[web] theme_color` escaped for
their context, and asks for `viewport-fit=cover`, which the host's safe area
needs. The manifest a build writes for a project that keeps none and the one
`flui platform add web` scaffolds come from one renderer
(`web_page::manifest`), which writes JSON through `serde_json`.

A project's page is never overridden in silence. An `index.html` in a
project's web platform directory that `[web] page` does not name stops the
build before anything compiles, with the two ways on: delete it, or name it
as the page template. `[web] page`, a kept page and the project's manifest
and icons are resolved against the directory of `flui.toml`, wherever the
build runs from. A template holds `{{flui_module}}`, which becomes the script
that starts the app (the build checks that it is there, not that it is
outside a comment), and may hold `{{flui_base}}`, `{{flui_manifest}}`,
`{{flui_stylesheet}}`, `{{flui_title}}`, `{{flui_description}}` and
`{{flui_theme_color}}`. Placeholders are replaced in one pass, left to right,
never inside a value the build put in, and values are escaped for HTML text
and quoted attribute values only (not for a script, a style or an unquoted
attribute), so a description that holds a placeholder's text stays
text (`a_value_holding_a_placeholder_stays_text`). `[web] theme_color` is a
`ThemeColor`, a hex colour or a CSS named colour from the list of CSS Color 4,
checked when `flui.toml` loads.

`flui run`'s server (`serve.rs`) serves the build under the same base path
and answers a navigation to a path whose last segment has no extension (a
trailing slash included) and that names no file with the page, which history
routing needs. A navigation is a request whose `Sec-Fetch-Mode` is
`navigate`, or, from a browser that sends none, whose `Accept` names HTML;
a `fetch()` (`Sec-Fetch-Mode: cors`) is none. It serves nothing outside the
build directory: the path is percent-decoded, a component that is not a plain
name or that holds a backslash, a colon or a NUL is refused, and a file is
opened only when its canonical path is still inside the directory and is
spelled as the request spells it (so no type is decided on another case of a
name). The `Host` check runs on a whole head with exactly one `Host`: a head
that reaches `MAX_HEAD` without its end is 431, none or two `Host`s 400, and
a `Host` other than the loopback (`localhost`, `127.0.0.1`, `[::1]`) 421,
against a page whose name is rebound to 127.0.0.1; with the method check, it
runs before any path's own answer, the reload poll's included. It serves at
most `MAX_CONNECTIONS` connections at once, of which at most `MAX_POLLS`
reload long polls; one poll past the second bound is answered at once with
the current generation (a short poll, which the page asks again a second
later). One connection past the first bound is answered 503 on a thread of
its own, counted until it closes, at most `MAX_REFUSALS` at once for at most
`REFUSAL` each, and past those closed unanswered, so the accepting thread
never waits on a refused client; a refused client has its request bytes read
before the close, so it reads the status. The page is served with the
generation it is (`reload.js?generation=N`), which its first poll waits past,
so a rebuild before that poll still reloads it. It cuts a request head off
at `REQUEST_HEAD` however slowly it arrives, and a response off at
`RESPONSE_WRITE` however slowly it is read (the socket's timeout is set again
only once the time left has shrunk by `WRITE_TIMEOUT_SLACK`), so a client that
stops reading gives its place back. Only a regular file is served: a FIFO, a
socket or a device is a 404 before it is opened, and on Unix the open is
non-blocking, since opening a FIFO for reading waits for a writer and would
hold the place for good. A file of the build is copied from disk as it is
written, so a large module is never held in memory once per connection; the
page, read whole to carry the reload script, is answered 500 past `MAX_PAGE`
before it is read. It sends `X-Content-Type-Options: nosniff`. A missing asset
and a request that is not a navigation get a 404 (`dev_server_contract`).

## The watcher is dev-machine code

`SourceWatcher` (`src/watch.rs`) watches files on the developer's machine;
nothing in a running app does. Keeping it here is what lets
`cargo install flui-cli` compile no framework code: the runtime half of hot
reload, `flui-hot-reload`, is linked by the app, and the two halves share
only a pair of environment-variable names.

A change is a write, a creation, a removal or a rename, never an open or a
close: a build reads every source and the manifest, and Linux reports each read
(`reading_the_sources_is_no_change`). A change is reported once no other has
come for the debounce window, at most two seconds after it began, and the
events that waited while the caller rebuilt join it, so one rebuild answers
them (`the_batches_waiting_for_a_busy_caller_are_one_change`). A file is
watched through its directory, filtered to its name, since an editor that
saves by rename replaces the inode a file watch sits on
(`a_watched_file_saved_by_rename_stays_watched`); a backend that lost events
reports every watched path.

### Finishing a probe transfers its captured output

The pipe reader owns a mutex-protected byte buffer shared with its drain thread.
At the deadline, finishing takes those bytes under the same lock rather than
cloning an entire output stream. A detached reader may subsequently append to
the empty buffer; it cannot change the returned snapshot. The existing
`grandchild_holding_the_pipe_does_not_hang_the_probe` checks the deadline and
captured prefix on Unix. CLI output integration tests check the delivered bytes;
no allocation-count claim is inferred from those behavior tests.

### Build selectors reach the selected platform unit

Web builds forward `--example` and `--package` through the existing Cargo target
resolver. Android APK builds compile the current package's library as its cdylib;
unsupported selectors are rejected before SDK probing rather than ignored.
`platform_build_selectors_reach_the_selected_cargo_unit_or_refuse` drives the
public CLI. Its portable fixture delegates metadata and project discovery to
real Cargo, substitutes tool availability, and refuses compilation while
reporting the actual selected Cargo arguments. It verifies selection without
claiming a wasm compilation or Android delivery run.


### Generated crate names respect Rust's reserved edition keywords

Project names become bare crate identifiers in generated application source,
so validation refuses `gen` and `try` along with the other reserved keywords.
`create_with_an_invalid_project_name_is_rejected` includes public CLI rows for both names;
accepting either would create source that the selected Rust edition rejects.

### Worker staging uses durable content identity

Staged worker paths include the full SHA-256 digest of the exact bytes read for
that build. The host can retain a dynamic-loader mapping after a staged file is
pruned, so a later path reuse is valid only for identical content. SHA-256's
cryptographic collision resistance is the deliberate content-identity assumption;
the old 32-bit FNV address space admitted practical collisions and silently
served an earlier image. An existing path is also checked against the built bytes.
Differing or interrupted staging is refused without overwriting a potentially
loaded image. Removing the corrupt staged copy permits the next admission.

`commands::run::tests::run_contract` includes
`staged_worker_content_identity_survives_reuse_and_pruning` with two distinct
images that collide under the previous hash, unchanged-file reuse and a pruned
image's return. `corrupt_staged_worker_is_refused_without_overwriting_a_loaded_path`
pins refusal, preservation of both files and the next staging operation. These
private orchestration seams exercise the actual writer the host consumes without
requiring a compiled plugin or platform loader; they do not certify dyld unloading.

Staging publishes a fully written same-directory temporary file with no-clobber
persistence. Concurrent identical admissions verify and reuse the first complete
snapshot instead of truncating it. The published bytes preserve cargo's source
signature and are never re-signed. `concurrent_identical_workers_publish_one_complete_immutable_snapshot`
coordinates three actual staging calls at the private pre-publication seam and
checks unpublished completeness, shared final identity and next-operation reuse.
