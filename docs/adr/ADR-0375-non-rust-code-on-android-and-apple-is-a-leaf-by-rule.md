# ADR-0375: Non-Rust code on Android and Apple is a leaf, by rule

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0154](ADR-0154-capability-crates.md) decision 5, items 2 and 3 (the
  checked-in dex only for Java that needs framework APIs, and a Gradle library module for a
  manifest component: a manifest component that needs only framework APIs ships in the checked-in
  dex, declared with its components, §2); [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decision 21
  where it differs (every crate that declares a dex has it checked, from a list the manifests
  give, with a protocol constant per dex, §3; the Gradle path writes a provider's authority from
  Gradle's application id, §4) and decision 8 in its list of what the bridge offers (it also runs
  a provider's work on Java's main thread, §6); [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md)
  §4's provider authority only (the Gradle path writes `${applicationId}.<suffix>`, §4)
- **Related:** [ADR-0247](ADR-0247-android-java-view.md) (the first checked-in dex),
  [ADR-0307](ADR-0307-platform-declarations-are-crate-metadata.md) (where a crate declares its dex,
  its sources and its components), [ADR-0340](ADR-0340-the-android-activity-forwards-what-has-a-consumer-and-routes-results-by-token-and-epoch.md)
  §2 (every closure the host posts to Android's main thread goes through one function),
  [ADR-0366](ADR-0366-an-android-activity-runs-while-it-is-on-screen-and-a-process-runs-one.md)
  (one Activity runs at a time), [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  §3 (`android-dex` stays a check of generated files)
- **Sources:** `slint-ui/slint@af1798f58`, `makepad/makepad@506ec0510`,
  `project-robius/robius@729692cc6`, `tauri-apps/wry@55289ad74`, `emilk/egui@9c2080470`,
  `linebender/xilem@dc496876a`, `rust-mobile/android-activity@b4ddf059b`,
  `azw413/smali@0f97dd326`, `jni-rs/jni-rs@dbfb17647`, `jni-rs/jbindgen@b331bcc2c`,
  `GrapheneOS/grapheneos.org@19f99166b`, and the issues cited inline

## Context and problem statement

FLUI and its packages are Rust, but two platforms keep some of what an application needs behind
another language. On Android a class Android instantiates by name (an `Activity`, a `View`, a
`ContentProvider` the manifest names), an override of a framework method
(`onCreateInputConnection`) and the handler of a listener are Java objects; JNI cannot define a
class on Android ("Android doesn't implement `DefineClass`",
[jni-rs/jni-rs#340](https://github.com/jni-rs/jni-rs/issues/340)), and a class must be in the
APK's own dex to be started by the system
([rust-mobile/android-activity#174](https://github.com/rust-mobile/android-activity/issues/174),
[#53](https://github.com/rust-mobile/android-activity/issues/53)). On Apple a few frameworks (App
Intents, ActivityKit, SwiftUI-only API) are Swift-only.

The rules in force disagreed with the code. ADR-0154 decision 5 allowed a checked-in dex only for
Java that needs framework APIs and sent a manifest component to a Gradle library module, while
`flui-system` ships `FluiDocuments`, a `ContentProvider`, in its checked-in dex with an
`android.providers` declaration (ADR-0307 §2). ADR-0255 decision 21 named one dex
(`flui-native`'s), while the digest gate and its workflow kept a list of crates by hand. The
provider's authority was fixed at build time from `flui.toml`'s id while the Java side computed
it from the package name at run time, so a Gradle `applicationIdSuffix` broke every view and
share. And a capability crate posted its own `Runnable` to Java's main thread, a second road
beside the host's, without its deadline or its refusal (ADR-0340 §2).

Which non-Rust code may FLUI and its packages ship on Android and Apple, how does it reach an
application's build, and how are its calls into the platform written?

## Decision drivers

- No JDK, Android build tools or Xcode in a user's build (ADR-0154 decision 5's last paragraph,
  ADR-0255 decision 21): `flui build` packs what the crates the application links declare.
- What can be Rust is Rust: every line of Java or Swift is a second language to review, a second
  toolchain for whoever edits it, and a binary under review.
- A class the platform must instantiate by name cannot be made from Rust on Android, and a
  `java.lang.reflect.Proxy` implements interfaces only
  ([`Proxy`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/reflect/Proxy.html)).
- Dex loaded from memory or a file at run time is refused where dynamic code loading is
  restricted (GrapheneOS lets a user block it per application:
  `GrapheneOS/grapheneos.org@19f99166b static/features.html:436-455`;
  [slint-ui/slint#14877](https://github.com/slint-ui/slint/issues/14877), open, closes Slint
  applications there).
- One fact in one place: the list of crates that ship a dex is their manifests'.
- Behaviour a crate needs from Java's main thread has one owner, the host, with one deadline and
  one refusal (ADR-0340 §2).
- Pre-1.0 the hand-written JNI signatures are checked by no compiler; a typed binding would be.

## Considered options

1. Hand-written Java per feature, compiled where it is used (Slint, Makepad, Robius, Tauri).
2. Plain `NativeActivity` and no Java at all (egui, Xilem).
3. Dex loaded in memory at run time (`InMemoryDexClassLoader`, Slint's delivery).
4. Dex written from Rust as smali (the `smali` crate), no Java source.
5. Gradle only: every Java class in a Gradle library module of its crate.
6. The leaf rule decided below: Java only where the platform requires a class, as the owning
   crate's checked-in dex declared in its metadata; calls and listeners in Rust.

## Decision

Chosen option: 6, because it is the only one that keeps the features a class is required for
(input methods, drag and drop, content providers, activity results) without putting a JDK into a
user's build or loading code at run time, and it holds the non-Rust code to the few classes the
platform forces.

### §1. What may be non-Rust

FLUI and its packages are Rust. A platform runtime that cannot be driven from Rust gets the
smallest leaf that makes it drivable, and nothing more:

- **Android: Java**, only for a subclass or override of a framework class (`FluiActivity extends
  NativeActivity`, `FluiView`, `FluiInputConnection`) or a component the manifest must name
  (`FluiDocuments extends ContentProvider`). Never for a call into the platform, which is JNI from
  Rust, and never for a listener interface, which is a `java.lang.reflect.Proxy` whose one
  `InvocationHandler` (`dev.flui.system.FluiCallback` in `flui-system`) forwards every method to
  one native function. A leaf class holds no logic Rust could hold: it forwards to native code
  and answers from what Rust published.
- **Apple: Swift**, only for a Swift-only framework, as a Swift package in the capability crate's
  `ios/` directory, built only for an application that links the crate (ADR-0255 decision 15
  stands). Everything Objective-C reaches is Rust over `objc2`.

### §2. How Android Java ships

Java that uses only framework APIs, a manifest component included, ships as the owning crate's
checked-in dex: the sources under the crate's `android/java`, the compiled `classes.dex` and its
record beside them, all declared in the crate's metadata (ADR-0307):
`[package.metadata.flui.declares.android]` `java`, `dex`, `proguard`, and the components it
contributes (`android.providers`, the first key of the kind; another kind of component gets its
key with its first consumer). `flui build` adds each declared dex to the APK as the next
`classesN.dex`, so the Activity's class loader loads it and the system can start the components;
a Gradle build gets the sources and the rules. No `javac`, `d8` or JDK runs in a user's build.

Java that needs a library outside the platform (AndroidX, Play Services) or resources is a Gradle
library module of its crate, and the crate's platform row says "Android (Gradle)".

### §3. One check over every dex, from the manifests

`cargo xtask android-dex` checks, for every workspace crate that declares `android.dex` and
`android.java` (at the top or under a feature), that the record's digests match the sources and
the dex; the list comes from `cargo metadata`, never from a list kept by hand, and the `Android
dex` workflow runs on any `android/java` tree and uploads what `--list` names. Each crate's Java
declares a protocol constant (`FluiHost.PROTOCOL`, `FluiCallback.PROTOCOL`) that its Rust compares
with its own before it binds a native or calls a method, and refuses with a typed error naming
both values; a provider that needs its crate's classes answers `NotDeclared` when they are
missing or of another protocol.

### §4. A component's authority is the application's id as the run knows it

A provider's authority is `<application id>.<suffix>`. The Gradle path writes it as
`${applicationId}.<suffix>`, which Gradle replaces with the id of the variant it builds (a suffix
included, so a debug and a release install never claim one authority); the SDK-tools path writes
the id `flui.toml` gives, the package it links. The Java side never computes its authority: it
keeps the one Android attached it with (`ContentProvider.attachInfo`'s `ProviderInfo`).

### §5. Calls into the SDK

Calls from Rust into the Android SDK go through `jni`. The direction is typed bindings over
hand-written signature strings: `jni` 0.22's `bind_java_type!` (`jni-rs/jni-rs@dbfb17647
crates/jni/docs/macros/bind_java_type_0_overview.md`), generated from `android.jar` by `jbindgen`
(`jni-rs/jbindgen@b331bcc2c README.md:22-26`) and checked in, so a misspelt method or descriptor
fails to compile. The migration of the hand-written calls is open work
([`docs/gaps.md`](../gaps.md#jni-calls-are-hand-written-signature-strings)); new code follows the
form of the crate it lands in until that crate migrates.

### §6. Work for Java's main thread goes through the host

A capability crate that needs Java's main thread (starting an activity, showing a dialog) hands
the work to the host: `flui_platform::capability::Activity::on_main_thread`, which the host runs
inside its panic boundary through its one posting function (ADR-0340 §2) and answers through a
`Completion<Result<T, MainThreadRefused>>`: the work's answer, or `ActivityEnded`,
`TooMuchPosted` (past `MAX_POSTED_WORK`) or `HandOffFailed` (the main thread did not take it
within the host's limit, after which it never runs, or it panicked). A crate posts nothing to
that thread by itself.

### Consequences

- Good, because a user's build needs neither a JDK nor Xcode, and no code is loaded at run time,
  so a device that blocks dynamic code loading runs FLUI.
- Good, because the Java is a list of classes the platform forces, each forwarding to Rust, and
  the gate's list cannot drift from the crates that ship a dex.
- Good, because a Gradle variant's suffixed id serves its own documents, and two installs never
  collide on an authority.
- Good, because a provider's main-thread work has the host's deadline and refusal, and a work the
  main thread never takes ends instead of holding its caller.
- Bad, because whoever edits a crate's Java needs a JDK and the recorded build tools, or the
  workflow; the dex is a binary under review, held to its sources only by digests.
- Bad, because the hand-written JNI calls stay unchecked by any compiler until §5's migration.
- Neutral, because a feature that needs AndroidX still takes the Gradle route, at its stated cost.

### Confirmation

- `cargo xtask android-dex` (in `cargo xtask checks`) and its table test `android_dex_contract`:
  `the_trees_are_what_the_crates_declare` (the list is the manifests', a feature's dex included, a
  half-declared pair refused) and `the_checked_in_dex_matches_its_sources`.
- `flui-system`'s `the_java_classes_state_what_the_library_and_the_manifest_name` (one protocol,
  the bound natives, the provider class the manifest declares, an authority taken from
  `attachInfo` and never from the package name).
- `flui-cli`'s `the_application_declares_the_providers_its_crates_ship` (the SDK-tools path's
  literal id and the Gradle path's `${applicationId}`) and
  `the_doctor_finds_what_the_manifest_carries`.
- `flui-platform`'s `work_for_the_main_thread_answers_through_the_host` (the work's answer or the
  host's refusal, settled once at an owner turn).
- The leaf rule of §1 itself (no Java for a call or a listener, no Swift outside a capability
  crate's `ios/`) is read by the reviewer of each change (ADR-0371).

## Pros and cons of the options

### 1. Hand-written Java per feature, compiled where it is used

Slint ships one helper class and compiles it from its `build.rs` with `javac` and `d8`
(`slint-ui/slint@af1798f58 internal/backends/android-activity/build.rs:18-65`); Makepad ships 17
Java files and compiles them with a JDK it downloads (`makepad/makepad@506ec0510
tools/cargo_makepad/src/android/compile.rs:173,1357,1391-1406`); Robius compiles each capability
crate's Java in its `build.rs` (`project-robius/robius@729692cc6 crates/file-picker/build.rs:20,35-51`);
wry ships Kotlin into the application's Gradle project (`tauri-apps/wry@55289ad74 src/android/kotlin/`).

- Good, because any feature can be written in Java, where Android's samples are.
- Bad, because the JDK and the build tools are in every user's build and its versions decide the
  output (Slint's `build.rs:69-71` checks the `javac` version for that reason).
- Bad, because nothing bounds how much logic moves into Java.

### 2. Plain `NativeActivity`, no Java

egui and Xilem run on `NativeActivity` through winit and `android-activity` with no Java
(`emilk/egui@9c2080470 crates/eframe/Cargo.toml:50-52`, `linebender/xilem@dc496876a
xilem/examples/android/README.md`).

- Good, because there is no second language and no dex at all.
- Bad, because `NativeActivity` "doesn't have built-in support for input methods"
  (`rust-mobile/android-activity@b4ddf059b README.md:195-197`), and overriding a method of the
  running Activity from Rust is not possible
  ([rust-mobile/android-activity#66](https://github.com/rust-mobile/android-activity/issues/66)):
  no input connection, drag and drop, content receiving or activity results.

### 3. Dex loaded in memory at run time

- Good, because the dex rides inside the native library and no packager handles it.
- Bad, because a device that restricts dynamic code loading refuses it
  ([slint-ui/slint#14877](https://github.com/slint-ui/slint/issues/14877)), and a component the
  system starts by name cannot live there at all (#174). ADR-0255 decision 4 already removed it.

### 4. Dex written from Rust as smali

- Good, because no JDK is needed by anyone; the `smali` crate is "completely self-contained and
  does not rely on any Java based dependencies" (`azw413/smali@0f97dd326 README.MD:14`).
- Bad, because the classes become hand-written Dalvik assembly, harder to read and review than
  Java, from a single-author crate whose output no FLUI run has checked on ART.

### 5. Gradle only

- Good, because it is Android's own route, with AAR libraries and manifest merging.
- Bad, because Gradle and a JDK become part of every Android build, which `flui build`'s SDK-tools
  path exists to avoid.

### 6. The leaf rule

- Good, because the features a class is required for stay, with no JDK in a user's build and no
  run-time loading.
- Good, because the rule is small enough to read in review: a subclass, an override or a manifest
  component, and nothing else.
- Bad, because a maintainer who edits the Java needs the recorded JDK and build tools or the
  workflow, and the JNI calls stay hand-written until §5's migration.

## More information

- jni-rs/jni-rs#340 (closed): JNI cannot construct a subclass, and "Android doesn't implement
  `DefineClass`"; the recommended route is a compiled class instantiated over JNI.
- rust-mobile/android-activity#53 (open): there is no `NativeService`; a service is a Java class
  in the APK's dex. #66 (closed): a method of the running Activity cannot be overridden from Rust;
  the application subclasses it in Java. #174 (open): an Activity loaded through a run-time class
  loader is not one the system starts; it must be in the application's own `classes.dex`.
- Revisit §5 when `jbindgen` binds `android.jar` without a JDK on the generating machine for the
  classes FLUI calls, and §1 if Android gains a way to define a class from native code.
