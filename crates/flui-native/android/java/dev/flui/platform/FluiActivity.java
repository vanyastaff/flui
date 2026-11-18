package dev.flui.platform;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.util.Log;
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
 * queue in {@link FluiHost}. A process runs one, in a task of its own
 * application: another instance forwards what it was opened with there and
 * finishes (ADR-0366).
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
        Intent received = getIntent();
        // Before `NativeActivity.onCreate` starts `android_main`, whose
        // application runs only over the Activity that owns the host's
        // queues: a process runs one FLUI platform, in a task of its own
        // application (ADR-0366).
        int admission =
                FluiHost.admit(this, isTaskRoot(), FluiHost.isForwarded(received));
        // Also before `android_main`: a provider that asks whether an earlier
        // answer is still to come finds the requests the saved state names.
        // Only the owner's: no other instance answers them.
        if (admission == FluiHost.OWN && savedInstanceState != null) {
            FluiHost.restore(savedInstanceState);
        }
        super.onCreate(savedInstanceState);
        if (admission != FluiHost.OWN) {
            leave(admission, received, savedInstanceState == null);
            return;
        }
        // `NativeActivity.onCreate` has just set this same mode
        // (`SOFT_INPUT_STATE_UNSPECIFIED | SOFT_INPUT_ADJUST_RESIZE`). It is
        // set again here, beside the edge-to-edge setup that relies on it, so
        // the mode the inset handling needs is FLUI's own statement and not
        // an inherited default; it changes nothing on today's levels.
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

    /**
     * An instance that does not run FLUI leaves ({@link FluiHost#admit}). One
     * that would run it in another application's task, or beside a live
     * owner, starts this Activity again in its own application's task with
     * what it was opened with ({@link FluiHost#FORWARD}): the start grants
     * the running instance the URIs this one was granted, and reaches it
     * through {@code onNewIntent}. One that was already forwarded and still
     * finds a live owner queues what it was opened with for the owner and
     * brings the owner's task to the front ({@link FluiHost#HAND_OVER}).
     */
    private void leave(int admission, Intent received, boolean fresh) {
        // A recreated instance, and one Recents starts again, carry the intent
        // that opened before: they bring the application forward with nothing.
        boolean opens = fresh
                && received != null
                && (received.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) == 0;
        if (admission == FluiHost.FORWARD) {
            try {
                startActivity(FluiHost.forwarded(this, opens ? received : null));
            } catch (RuntimeException error) {
                // A URI this instance may not grant (`SecurityException`):
                // the owner reads it as far as the grant reaches.
                Log.w(TAG, "the intent could not be forwarded; it is handed over", error);
                handOver(opens ? received : null);
            }
        } else {
            handOver(opens ? received : null);
        }
        finish();
    }

    /** {@code received}, if any, waits for the owner, whose task comes forward. */
    private void handOver(Intent received) {
        if (received != null) {
            FluiHost.opened(received, false);
        }
        FluiHost.showOwner(this);
    }

    private static final String TAG = "flui";

    /** Below API 30: the layout flags, added to the decor view's. */
    @SuppressWarnings("deprecation")
    private static void layoutEdgeToEdge(Window window) {
        View decor = window.getDecorView();
        decor.setSystemUiVisibility(decor.getSystemUiVisibility() | EDGE_TO_EDGE_LAYOUT_FLAGS);
    }

    /** What API 30 adds. */
    private static final class Api30 {
        // Deprecated from API 35, where an app targeting it is edge to edge
        // already and the call changes nothing; below that target it is the
        // call that makes the window edge to edge.
        @SuppressWarnings("deprecation")
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
