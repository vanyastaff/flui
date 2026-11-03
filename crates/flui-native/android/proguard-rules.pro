# R8 rules for FLUI's Java classes, which `flui build` hands a Gradle build as
# `flui.proguardFiles` (ADR-0307 version 2, `android.proguard`).
#
# The native library reaches these classes by name, never through code R8
# sees: it loads them by binary name through the Activity's class loader,
# reads `FluiHost.PROTOCOL`, calls their methods by name and signature
# through JNI, and binds their native methods with `RegisterNatives`
# (ADR-0255 decisions 4 and 5). A class, method or field R8 renamed or
# removed would fail the handshake or a call at run time, so all of them
# stay as written.
-keep class dev.flui.** { *; }
