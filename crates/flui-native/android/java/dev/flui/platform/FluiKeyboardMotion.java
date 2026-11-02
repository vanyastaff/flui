package dev.flui.platform;

import android.view.WindowInsets;
import android.view.WindowInsetsAnimation;
import android.view.animation.Interpolator;
import java.util.List;

/**
 * {@link FluiView}'s observer of the input method's insets animation (API 30).
 *
 * The view is laid out at the animation's end before {@link #onStart}, so the
 * native side already reads the keyboard's target; this reports how it gets
 * there: the bottom edge before and after, when the animation started, its
 * length and three values of its interpolator, from which the native side
 * names the curve. The native side samples that motion on its own frame clock.
 * Each frame of the animation checks the clock the motion was reported on,
 * and reports it again, started where the animation's own fraction puts it,
 * when the two drift apart by more than a frame. The end is reported too, so
 * the native side reads the keyboard at rest again.
 *
 * Its own class, loaded only on API 30 and later: the class it extends does
 * not exist before.
 */
final class FluiKeyboardMotion extends WindowInsetsAnimation.Callback {
    /** The fractions of time the interpolator is read at (`PROBES` on the native side). */
    private static final float[] PROBES = {0.25f, 0.5f, 0.75f};
    /** How far the reported start may drift from the animation's before it is reported again. */
    private static final long DRIFT_NANOS = 16_000_000L;

    private final FluiView view;
    /** The keyboard's bottom edge before the animation now running, from {@link #onPrepare}. */
    private int from;
    /** The keyboard's bottom edge once that animation ends. */
    private int to;
    /** Its interpolator's values at {@link #PROBES}. */
    private final float[] values = new float[PROBES.length];
    /** The start last reported, on {@link System#nanoTime}'s clock; 0 when none is. */
    private long start;
    private long durationMillis;
    /**
     * The keyboard animation last started: the end of one it replaced, which
     * may arrive after the new one's start, settles nothing.
     */
    private WindowInsetsAnimation running;

    private FluiKeyboardMotion(FluiView view) {
        super(DISPATCH_MODE_STOP);
        this.view = view;
    }

    static void install(FluiView view) {
        view.setWindowInsetsAnimationCallback(new FluiKeyboardMotion(view));
    }

    @Override
    public void onPrepare(WindowInsetsAnimation animation) {
        if (isKeyboard(animation)) {
            from = bottom();
        }
    }

    @Override
    public WindowInsetsAnimation.Bounds onStart(WindowInsetsAnimation animation,
            WindowInsetsAnimation.Bounds bounds) {
        if (!isKeyboard(animation)) {
            return bounds;
        }
        running = animation;
        to = bottom();
        durationMillis = animation.getDurationMillis();
        Interpolator curve = animation.getInterpolator();
        for (int index = 0; index < PROBES.length; index++) {
            values[index] = curve == null ? PROBES[index] : curve.getInterpolation(PROBES[index]);
        }
        report(System.nanoTime());
        return bounds;
    }

    @Override
    public WindowInsets onProgress(WindowInsets insets, List<WindowInsetsAnimation> animations) {
        if (start == 0 || durationMillis <= 0) {
            return insets;
        }
        for (WindowInsetsAnimation animation : animations) {
            if (animation == running) {
                long elapsed = (long) (animation.getFraction() * durationMillis * 1_000_000L);
                long started = System.nanoTime() - elapsed;
                if (Math.abs(started - start) > DRIFT_NANOS) {
                    report(started);
                }
                break;
            }
        }
        return insets;
    }

    @Override
    public void onEnd(WindowInsetsAnimation animation) {
        if (animation != running) {
            return;
        }
        running = null;
        start = 0;
        long handle = view.handle();
        if (handle != 0) {
            FluiView.nativeKeyboardSettled(handle);
        }
    }

    private void report(long started) {
        start = started;
        long handle = view.handle();
        if (handle != 0) {
            FluiView.nativeKeyboardMotion(handle, from, to, started, durationMillis,
                    values[0], values[1], values[2]);
        }
    }

    private static boolean isKeyboard(WindowInsetsAnimation animation) {
        return (animation.getTypeMask() & WindowInsets.Type.ime()) != 0;
    }

    /** The keyboard's bottom edge in the view's window now, in pixels. */
    private int bottom() {
        WindowInsets insets = view.getRootWindowInsets();
        return insets == null ? 0 : insets.getInsets(WindowInsets.Type.ime()).bottom;
    }
}
