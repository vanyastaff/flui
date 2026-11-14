package dev.flui.platform;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Bundle;

/**
 * The Activity of a FLUI application (ADR-0255 decision 1).
 *
 * A {@link NativeActivity}: the native library takes the window's surface and
 * reads input from the native input queue, as before. Naming this class in
 * the manifest, with {@code android:hasCode="true"}, puts FLUI's classes in
 * the APK, where the Activity's class loader finds them, and gives FLUI the
 * callbacks only an Activity subclass receives, which it forwards to the
 * queue in {@link FluiHost}.
 */
public class FluiActivity extends NativeActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Intent intent = getIntent();
        // A recreated Activity, and one Recents starts again, carry the
        // intent that launched them before: it opened then.
        if (savedInstanceState == null
                && intent != null
                && (intent.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) == 0) {
            FluiHost.opened(intent, true);
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        // `getIntent()` answers the latest intent from here on.
        setIntent(intent);
        FluiHost.opened(intent, false);
    }

    @Override
    public void onTrimMemory(int level) {
        super.onTrimMemory(level);
        FluiHost.trimMemory(level);
    }
}
