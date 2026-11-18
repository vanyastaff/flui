package dev.flui.system;

import android.util.Log;

import java.lang.ref.PhantomReference;
import java.lang.ref.ReferenceQueue;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.util.HashSet;
import java.util.Set;

/**
 * The one object Android calls {@code flui-system}'s native code back through: the handler
 * of a {@code java.lang.reflect.Proxy}, so a dialog's listeners need no class of their own.
 *
 * <p>The library makes every instance, through JNI, with a handle of its own, and binds
 * {@link #called} and {@link #released} after it has read {@link #PROTOCOL}. A call of an
 * interface method reaches {@link #called} with the method's name; the methods every object
 * has are answered here, since a proxy routes those to its handler too. The handle is the
 * library's until this object is collected: a phantom reference to it is enqueued then, and a
 * daemon thread of this class hands its handle to {@link #released}. (A {@code Cleaner} does
 * the same from API 33; the floor is API 23, and {@code finalize} runs with the object still
 * reachable from the finalizer.)
 */
public final class FluiCallback implements InvocationHandler {
    /**
     * The contract with the native library, over this package's classes; raised with every
     * change of a signature the library calls or binds.
     */
    public static final int PROTOCOL = 3;

    private static final String TAG = "FluiCallback";

    /** Where the collected callbacks' references arrive. */
    private static final ReferenceQueue<FluiCallback> COLLECTED = new ReferenceQueue<>();

    /** The references not yet enqueued, kept reachable until they are; guarded by itself. */
    private static final Set<Release> PENDING = new HashSet<>();

    /** The thread that releases collected handles; started with the first callback. */
    private static Thread releaser;

    private final long handle;

    FluiCallback(long handle) {
        this.handle = handle;
        track(this, handle);
    }

    /** The handle of a callback, reachable only through its phantom reference. */
    private static final class Release extends PhantomReference<FluiCallback> {
        final long handle;

        Release(FluiCallback callback, long handle) {
            super(callback, COLLECTED);
            this.handle = handle;
        }
    }

    /** Releases each collected callback's handle, for as long as the process lives. */
    private static final class Releaser implements Runnable {
        @Override
        public void run() {
            while (true) {
                Release release;
                try {
                    release = (Release) COLLECTED.remove();
                } catch (InterruptedException interrupted) {
                    return;
                }
                synchronized (PENDING) {
                    PENDING.remove(release);
                }
                try {
                    released(release.handle);
                } catch (Throwable thrown) {
                    // The native side contains its own failures; anything else
                    // must not end the thread every later release waits on.
                    Log.w(TAG, "a callback's handle was not released", thrown);
                }
            }
        }
    }

    private static void track(FluiCallback callback, long handle) {
        synchronized (PENDING) {
            PENDING.add(new Release(callback, handle));
            // Started with the first callback, and again should it have died.
            if (releaser == null || !releaser.isAlive()) {
                releaser = new Thread(new Releaser(), "flui-callback-release");
                releaser.setDaemon(true);
                releaser.start();
            }
        }
    }

    @Override
    public Object invoke(Object proxy, Method method, Object[] args) {
        if (method.getDeclaringClass() == Object.class) {
            switch (method.getName()) {
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    return "FluiCallback@" + Long.toHexString(handle);
            }
        }
        // The one argument the library reads: the button of `OnClickListener.onClick`.
        int argument = 0;
        if (args != null && args.length > 1 && args[1] instanceof Integer) {
            argument = (Integer) args[1];
        }
        called(handle, method.getName(), argument);
        return null;
    }

    /** The interface method {@code method} was called on the proxy of {@code handle}. */
    private static native void called(long handle, String method, int argument);

    /** Nothing calls {@code handle} any more. */
    private static native void released(long handle);
}
