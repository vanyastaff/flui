package dev.flui.platform;

import java.util.ArrayDeque;

/**
 * The contract between this dex and the native library (ADR-0255 decision 5),
 * and the queue that carries what {@link FluiActivity} receives and
 * {@code NativeActivity} drops (decision 2).
 *
 * <p>The library loads these classes through the Activity's class loader, from
 * the APK, so the dex and the library are built separately and can come from
 * different FLUI versions. Before {@code RegisterNatives}, the library reads
 * {@link #PROTOCOL} and compares it with its own constant; a mismatch stops
 * the start with an error naming both values, instead of an
 * {@code UnsatisfiedLinkError} or a {@code NoSuchMethodError} far from the
 * cause.
 *
 * <p>The queue buffers: an entry can arrive before {@code android_main} runs,
 * or between two platforms of one process. The library binds a wake handle on
 * the Java main thread once its platform starts, and unbinds it there when the
 * platform ends; while one is bound, each entry wakes the owner thread, which
 * drains the queue. The Java side queues and wakes and never waits for the
 * owner; the lock guards the deque alone, and neither side calls out while it
 * holds it.
 */
public final class FluiHost {
    /**
     * The version of the contract between these classes and the native
     * library: their native methods and signatures, the methods the library
     * calls, the entries of the queue, and what each means. Raised with every
     * change to any of them, together with the library's constant.
     */
    public static final int PROTOCOL = 2;

    /** An entry for {@code onTrimMemory}; its value is the level. */
    static final int TRIM_MEMORY = 1;

    private static final Object LOCK = new Object();

    /** Pairs of kind and value, oldest first. Guarded by {@link #LOCK}. */
    private static final ArrayDeque<int[]> ENTRIES = new ArrayDeque<>();

    /** The bound wake handle, {@code 0} for none. Java main thread only. */
    private static long wakeHandle;

    private FluiHost() {}

    /**
     * {@link FluiActivity#onTrimMemory}. A level already waiting is not
     * queued again, so the queue holds at most one entry per level.
     */
    static void trimMemory(int level) {
        synchronized (LOCK) {
            for (int[] entry : ENTRIES) {
                if (entry[0] == TRIM_MEMORY && entry[1] == level) {
                    return;
                }
            }
            ENTRIES.add(new int[] {TRIM_MEMORY, level});
        }
        wake();
    }

    private static void wake() {
        if (wakeHandle != 0) {
            nativeWake(wakeHandle);
        }
    }

    /**
     * Bind the library's wake handle; Java main thread. Entries queued before
     * wake it at once.
     */
    static void bind(long handle) {
        wakeHandle = handle;
        boolean waiting;
        synchronized (LOCK) {
            waiting = !ENTRIES.isEmpty();
        }
        if (waiting) {
            wake();
        }
    }

    /**
     * Forget {@code handle}; Java main thread. A handle a later platform bound
     * since stays. The library frees {@code handle} after this returns, so no
     * call passes it again.
     */
    static void unbind(long handle) {
        if (wakeHandle == handle) {
            wakeHandle = 0;
        }
    }

    /** Every entry queued, oldest first, as kind and value pairs; any thread. */
    static int[] drain() {
        synchronized (LOCK) {
            int[] pairs = new int[ENTRIES.size() * 2];
            int index = 0;
            for (int[] entry = ENTRIES.poll(); entry != null; entry = ENTRIES.poll()) {
                pairs[index++] = entry[0];
                pairs[index++] = entry[1];
            }
            return pairs;
        }
    }

    private static native void nativeWake(long handle);
}
