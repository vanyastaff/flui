package dev.flui.platform;

import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.os.Parcelable;
import android.provider.OpenableColumns;
import android.util.Log;

import java.util.ArrayDeque;
import java.util.ArrayList;

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
 * <p>The queues buffer: an entry, or what an intent opens, can arrive before
 * {@code android_main} runs, or between two platforms of one process. Entries
 * are pairs of kind and value ({@link #drain}); an intent is a record of its
 * launch, action and URIs ({@link #drainOpened}). The library binds a wake
 * handle on the Java main thread once its platform starts, and unbinds it
 * there when the platform ends; while one is bound, each entry and intent
 * wakes the owner thread, which drains both queues. The Java side queues and
 * wakes and never waits for the owner; the lock guards the deques alone, and
 * neither side calls out while it holds it.
 */
public final class FluiHost {
    /**
     * The version of the contract between these classes and the native
     * library: their native methods and signatures, the methods the library
     * calls, the entries of the queue, and what each means. Raised with every
     * change to any of them, together with the library's constant.
     */
    public static final int PROTOCOL = 3;

    /** An entry for {@code onTrimMemory}; its value is the level. */
    static final int TRIM_MEMORY = 1;

    /** The launch of an intent that started the Activity ({@code onCreate}). */
    static final String COLD = "cold";

    /** The launch of an intent that reached the running Activity ({@code onNewIntent}). */
    static final String RUNNING = "running";

    /**
     * The most intents that wait at once, and the most URIs one carries: the
     * library's {@code MAX_OPENED_ITEMS}, which bounds what it hears again.
     */
    static final int MAX_OPENED = 64;

    /**
     * The longest URI kept, in UTF-16 units: a URI within the library's
     * {@code MAX_OPENED_URL_LEN} bytes is never longer, so nothing the
     * library would take is lost, and nothing longer is copied across.
     */
    static final int MAX_URI_LENGTH = 8192;

    /** The longest name of a document kept, in UTF-16 units. */
    static final int MAX_NAME_LENGTH = 1024;

    /**
     * What each intent opens, oldest first, as {@link #record} lays it out.
     * Guarded by {@link #LOCK}; at most {@link #MAX_OPENED}.
     */
    private static final ArrayDeque<String[]> OPENED = new ArrayDeque<>();

    private static final String TAG = "flui";

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

    /**
     * {@link FluiActivity}'s {@code onCreate} ({@code cold}) and
     * {@code onNewIntent}: what {@code intent} opens waits for the owner. An
     * intent past {@link #MAX_OPENED} waiting ones is dropped, and logged.
     */
    static void opened(Intent intent, boolean cold) {
        String[] record = record(intent, cold);
        boolean kept;
        synchronized (LOCK) {
            kept = OPENED.size() < MAX_OPENED;
            if (kept) {
                OPENED.add(record);
            }
        }
        if (!kept) {
            Log.w(TAG, "more intents wait than FLUI holds; this one is dropped");
            return;
        }
        wake();
    }

    /**
     * The launch ({@link #COLD} or {@link #RUNNING}), the action ({@code ""}
     * for none), then the URIs of {@code intent}: its data, then each
     * {@code EXTRA_STREAM} of a share, at most {@link #MAX_OPENED}, each
     * within {@link #MAX_URI_LENGTH}. The extras come from the sender: one
     * that does not read as URIs adds none.
     */
    // `getParcelableExtra(String, Class)` begins at API 33; FLUI runs from 23.
    @SuppressWarnings("deprecation")
    static String[] record(Intent intent, boolean cold) {
        ArrayList<String> fields = new ArrayList<>();
        fields.add(cold ? COLD : RUNNING);
        String action = intent.getAction();
        fields.add(action == null ? "" : action);
        ArrayList<Object> uris = new ArrayList<>();
        uris.add(intent.getData());
        try {
            if (Intent.ACTION_SEND.equals(action)) {
                uris.add(intent.getParcelableExtra(Intent.EXTRA_STREAM));
            } else if (Intent.ACTION_SEND_MULTIPLE.equals(action)) {
                ArrayList<Parcelable> streams =
                        intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
                if (streams != null) {
                    uris.addAll(streams);
                }
            }
        } catch (RuntimeException error) {
            Log.w(TAG, "a share's streams do not read as URIs", error);
        }
        int skipped = 0;
        for (Object uri : uris) {
            if (!(uri instanceof Uri)) {
                continue;
            }
            String text = uri.toString();
            if (fields.size() - 2 < MAX_OPENED && text.length() <= MAX_URI_LENGTH) {
                fields.add(text);
            } else {
                skipped++;
            }
        }
        if (skipped > 0) {
            Log.w(TAG, skipped + " URIs of an intent are past what FLUI takes; they are dropped");
        }
        return fields.toArray(new String[0]);
    }

    /** Every intent's record queued, oldest first; any thread. */
    static String[][] drainOpened() {
        synchronized (LOCK) {
            String[][] records = OPENED.toArray(new String[0][]);
            OPENED.clear();
            return records;
        }
    }

    /**
     * The descriptor of the document {@code uri} names, opened for reading
     * through {@code context}'s content resolver and detached, so that the
     * library owns and closes it; {@code -1}, logged, when the provider
     * refuses or fails. The library calls it on its own thread: a provider
     * can take long.
     */
    static int openDocument(Context context, String uri) {
        try {
            ParcelFileDescriptor descriptor =
                    context.getContentResolver().openFileDescriptor(Uri.parse(uri), "r");
            return descriptor == null ? -1 : descriptor.detachFd();
        } catch (Exception error) {
            Log.w(TAG, "a document an intent opened could not be read", error);
            return -1;
        }
    }

    /**
     * The name the provider shows for the document {@code uri} names, or
     * {@code null} when it shows none, or one longer than
     * {@link #MAX_NAME_LENGTH}.
     */
    static String documentName(Context context, String uri) {
        try (Cursor cursor = context.getContentResolver().query(
                Uri.parse(uri), new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                String name = cursor.getString(0);
                return name.length() <= MAX_NAME_LENGTH ? name : null;
            }
        } catch (RuntimeException error) {
            Log.w(TAG, "a document's name could not be read", error);
        }
        return null;
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
            waiting = !ENTRIES.isEmpty() || !OPENED.isEmpty();
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
