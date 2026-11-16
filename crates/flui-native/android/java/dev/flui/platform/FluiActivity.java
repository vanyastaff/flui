package dev.flui.platform;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.Window;

/**
 * The Activity of a FLUI application (ADR-0255 decision 1).
 *
 * A {@link NativeActivity}: the native library takes the window's surface and
 * reads input from the native input queue, as before. Naming this class in
 * the manifest, with {@code android:hasCode="true"}, puts FLUI's classes in
 * the APK, where the Activity's class loader finds them, and gives FLUI the
 * callbacks only an Activity subclass receives, which it forwards to the
 * queue in {@link FluiHost}.
 *
 * <p>The request codes {@link FluiHost#FIRST_HOST_CODE} to
 * {@link FluiHost#LAST_HOST_CODE} are the host's: capability providers start
 * activities for results under them. An application that subclasses this
 * Activity and calls {@code startActivityForResult} itself uses codes below
 * them; its results reach its own {@code onActivityResult} untouched.
 */
public class FluiActivity extends NativeActivity {
    /**
     * The window's soft input mode on every level, {@code adjustResize}
     * ({@code SOFT_INPUT_STATE_UNSPECIFIED | SOFT_INPUT_ADJUST_RESIZE}), with
     * the window edge to edge, where Android dispatches the input method as
     * an inset and leaves the surface its size (ADR-0340). The library's
     * {@code owner::android::window_setup} holds this table, and its test
     * holds these constants to it.
     */
    static final int SOFT_INPUT_MODE = 16;

    /** The level from which the decor stops fitting the system windows. */
    static final int DECOR_DOES_NOT_FIT_FROM_API = 30;

    /**
     * Below {@link #DECOR_DOES_NOT_FIT_FROM_API}, the layout flags that put
     * the window edge to edge: {@code SYSTEM_UI_FLAG_LAYOUT_STABLE},
     * {@code _LAYOUT_HIDE_NAVIGATION} and {@code _LAYOUT_FULLSCREEN}.
     */
    static final int EDGE_TO_EDGE_LAYOUT_FLAGS = 1792;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // Before `NativeActivity.onCreate` starts `android_main`: a provider
        // that asks whether an earlier answer is still to come finds the
        // requests the saved state names.
        if (savedInstanceState != null) {
            FluiHost.restore(savedInstanceState);
        }
        super.onCreate(savedInstanceState);
        // After `NativeActivity.onCreate`, which sets its own mode, and before
        // the window's first layout.
        getWindow().setSoftInputMode(SOFT_INPUT_MODE);
        if (Build.VERSION.SDK_INT >= DECOR_DOES_NOT_FIT_FROM_API) {
            Api30.edgeToEdge(getWindow());
        } else {
            layoutEdgeToEdge(getWindow());
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

    /** Below API 30: the layout flags, added to the decor view's. */
    @SuppressWarnings("deprecation")
    private static void layoutEdgeToEdge(Window window) {
        View decor = window.getDecorView();
        decor.setSystemUiVisibility(decor.getSystemUiVisibility() | EDGE_TO_EDGE_LAYOUT_FLAGS);
    }

    /** What API 30 adds. */
    private static final class Api30 {
        static void edgeToEdge(Window window) {
            window.setDecorFitsSystemWindows(false);
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
