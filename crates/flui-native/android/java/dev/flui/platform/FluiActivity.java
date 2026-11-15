package dev.flui.platform;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.view.WindowManager;

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
    /**
     * The window's soft input mode from API 30: the window never resizes for
     * the input method, whose height reaches FLUI as the {@code ime()}
     * insets and their animation instead. Below API 30 the insets carry the
     * input method only under {@code adjustResize}, which
     * {@code NativeActivity} sets, so the mode stays.
     */
    static final int SOFT_INPUT_FROM_API_30 =
            WindowManager.LayoutParams.SOFT_INPUT_STATE_UNSPECIFIED
                    | WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // `NativeActivity.onCreate` sets `adjustResize` on the window itself,
        // so the manifest's attribute would change nothing.
        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().setSoftInputMode(SOFT_INPUT_FROM_API_30);
        }
        if (savedInstanceState != null) {
            FluiHost.restore(savedInstanceState);
        }
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
    protected void onStart() {
        super.onStart();
        FluiHost.started(this);
    }

    @Override
    protected void onStop() {
        FluiHost.stopped(this);
        super.onStop();
    }

    @Override
    protected void onDestroy() {
        FluiHost.destroyed(this);
        super.onDestroy();
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        FluiHost.save(outState);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        FluiHost.activityResult(requestCode, resultCode, data);
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
