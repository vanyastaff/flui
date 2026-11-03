package dev.flui.platform;

import android.app.NativeActivity;

/**
 * The Activity of a FLUI application (ADR-0255 decision 1).
 *
 * A {@link NativeActivity}: the native library takes the window's surface and
 * reads input from the native input queue, as before. Naming this class in
 * the manifest, with {@code android:hasCode="true"}, puts FLUI's classes in
 * the APK, where the Activity's class loader finds them, and gives FLUI the
 * callbacks only an Activity subclass receives.
 */
public class FluiActivity extends NativeActivity {}
