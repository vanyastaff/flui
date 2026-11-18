# R8 rules for flui-system's Java classes, which `flui build` hands a Gradle
# build as `flui.proguardFiles` (ADR-0307, `android.proguard`).
#
# The native library reaches these classes by name, never through code R8
# sees: it loads `FluiCallback` and `FluiDocuments` by binary name through
# the Activity's class loader, reads `FluiCallback.PROTOCOL`, constructs
# `FluiCallback` and calls `FluiDocuments`' methods by name and signature
# through JNI, and binds `FluiCallback`'s native methods with
# `RegisterNatives`; Android instantiates `FluiDocuments` by the name the
# manifest states. A class, method or field R8 renamed or removed would fail
# a call at run time, so all of them stay as written.
-keep class dev.flui.system.** { *; }
