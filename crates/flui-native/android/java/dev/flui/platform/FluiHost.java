package dev.flui.platform;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.os.Parcelable;
import android.provider.OpenableColumns;
import android.util.Log;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;

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
 * launch, action and URIs ({@link #drainOpened}); an answer to an activity
 * the library started for a result names the request's epoch, token and key
 * ({@link #drainAnswers}). The open requests are saved with the Activity's
 * state, so an answer that reaches a later Activity, or a later process,
 * still names its key. The library binds a wake
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
    public static final int PROTOCOL = 4;

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

    /** An answer: the activity a request started answered ({@code onActivityResult}). */
    static final int ANSWER_RESULT = 1;

    /** An answer: Android refused to start the request's activity, or it was not taken. */
    static final int ANSWER_NOT_LAUNCHED = 2;

    /** The numbers of one answer in {@link #drainAnswers}: epoch, token, kind, result code. */
    static final int ANSWER_NUMBERS = 4;

    /** The epoch of a request restored from the saved state of a process that died. */
    static final long EARLIER_PROCESS = 0;

    /**
     * The most requests open at once across the platforms of the process: waiting to start,
     * started, and answered but not drained; the library's {@code MAX_OPEN_REQUESTS}. A refusal
     * past it is an answer too, so at most twice as many answers wait.
     */
    static final int MAX_OPEN = 16;

    private static final String SAVED_CODES = "dev.flui.platform.requests.codes";

    private static final String SAVED_KEYS = "dev.flui.platform.requests.keys";

    private static final String SAVED_NEXT_CODE = "dev.flui.platform.requests.next";

    /**
     * A request the library took: the epoch of its platform ({@link #nextEpoch}), the token the
     * library minted, and the key that names its purpose across Activities.
     */
    private static final class Request {
        final long epoch;
        final long token;
        final String key;

        Request(long epoch, long token, String key) {
            this.epoch = epoch;
            this.token = token;
            this.key = key;
        }

        boolean is(long epoch, long token) {
            return this.epoch == epoch && this.token == token;
        }
    }

    /** A request whose Activity was stopped when it was taken, with what starts it. */
    private static final class Waiting {
        final Activity activity;
        final Intent intent;
        final Request request;

        Waiting(Activity activity, Intent intent, Request request) {
            this.activity = activity;
            this.intent = intent;
            this.request = request;
        }
    }

    /** What one request was answered with. */
    private static final class Answer {
        final Request request;
        final int kind;
        final int resultCode;
        /** The started activity's {@code Intent}, held, never read here. */
        final Intent data;

        Answer(Request request, int kind, int resultCode, Intent data) {
            this.request = request;
            this.kind = kind;
            this.resultCode = resultCode;
            this.data = data;
        }
    }

    /** Started requests by their request code. Java main thread only. */
    private static final HashMap<Integer, Request> STARTED = new HashMap<>();

    /** Requests taken while their Activity was stopped, oldest first. Java main thread only. */
    private static final ArrayDeque<Waiting> WAITING = new ArrayDeque<>();

    /** The next request code to try. Java main thread only. */
    private static int nextCode = 1;

    /** The Activity between its {@code onStart} and {@code onStop}. Java main thread only. */
    private static Activity startedActivity;

    /** Answers, oldest first. Guarded by {@link #LOCK}. */
    private static final ArrayDeque<Answer> ANSWERS = new ArrayDeque<>();

    /** The last epoch {@link #nextEpoch} gave. Guarded by {@link #LOCK}. */
    private static long lastEpoch;

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

    /**
     * The epoch of a platform's requests, a new one for each platform of the process and never
     * {@link #EARLIER_PROCESS}; any thread. An answer names the epoch of its request, so the
     * library tells its own Activity's answers from an earlier one's.
     */
    static long nextEpoch() {
        synchronized (LOCK) {
            lastEpoch++;
            return lastEpoch;
        }
    }

    /**
     * Start {@code intent} for a result from {@code activity}; Java main thread. The request is
     * the library's {@code token} under {@code key}, of the platform whose epoch is
     * {@code epoch}. While {@code activity} is stopped it waits for {@link #started}. Past
     * {@link #MAX_OPEN} open requests, or when Android refuses, it is answered with
     * {@link #ANSWER_NOT_LAUNCHED}.
     */
    static void launch(Activity activity, Intent intent, long epoch, long token, String key) {
        Request request = new Request(epoch, token, key);
        int answered;
        synchronized (LOCK) {
            answered = ANSWERS.size();
        }
        if (STARTED.size() + WAITING.size() + answered >= MAX_OPEN) {
            Log.w(TAG, "as many activity requests are open as FLUI holds; this one is refused");
            answer(new Answer(request, ANSWER_NOT_LAUNCHED, 0, null));
            return;
        }
        if (activity != startedActivity) {
            WAITING.add(new Waiting(activity, intent, request));
            return;
        }
        start(activity, intent, request);
    }

    private static void start(Activity activity, Intent intent, Request request) {
        int code = freeCode();
        STARTED.put(code, request);
        try {
            activity.startActivityForResult(intent, code);
        } catch (RuntimeException error) {
            // `ActivityNotFoundException`, `SecurityException`: nothing started.
            STARTED.remove(code);
            Log.w(TAG, "an activity FLUI was asked to start did not start", error);
            answer(new Answer(request, ANSWER_NOT_LAUNCHED, 0, null));
        }
    }

    /**
     * A positive request code no started request holds. Codes go up and start again at 1 after
     * {@code Integer.MAX_VALUE}: a code only tells Android's answer apart while its request is
     * started, and the library matches the answer by epoch and token.
     */
    private static int freeCode() {
        while (true) {
            int code = nextCode;
            nextCode = code == Integer.MAX_VALUE ? 1 : code + 1;
            if (!STARTED.containsKey(code)) {
                return code;
            }
        }
    }

    /**
     * {@link FluiActivity#onActivityResult}: the answer of a request FLUI started waits for the
     * owner; a code FLUI did not start is not FLUI's. {@code data} comes from another
     * application: it is held as it is, and only the provider that asked reads it.
     */
    static void activityResult(int code, int resultCode, Intent data) {
        Request request = STARTED.remove(code);
        if (request != null) {
            answer(new Answer(request, ANSWER_RESULT, resultCode, data));
        }
    }

    /**
     * The library cancelled its request {@code token} of the platform {@code epoch}; Java main
     * thread. One still waiting to start is forgotten; a started one is finished, and the
     * answer it then gives reaches the library, which drops it as answered.
     */
    static void cancel(Activity activity, long epoch, long token) {
        for (Iterator<Waiting> waiting = WAITING.iterator(); waiting.hasNext(); ) {
            if (waiting.next().request.is(epoch, token)) {
                waiting.remove();
                return;
            }
        }
        for (Map.Entry<Integer, Request> started : STARTED.entrySet()) {
            if (started.getValue().is(epoch, token)) {
                activity.finishActivity(started.getKey());
                return;
            }
        }
    }

    /** {@link FluiActivity#onStart}: the requests waiting for {@code activity} start. */
    static void started(Activity activity) {
        startedActivity = activity;
        for (int count = WAITING.size(); count > 0; count--) {
            Waiting waiting = WAITING.poll();
            if (waiting.activity == activity) {
                start(activity, waiting.intent, waiting.request);
            } else {
                WAITING.add(waiting);
            }
        }
    }

    /** {@link FluiActivity#onStop}. */
    static void stopped(Activity activity) {
        if (startedActivity == activity) {
            startedActivity = null;
        }
    }

    /**
     * {@link FluiActivity#onDestroy}: what waits to start from {@code activity} never will. Its
     * platform ends with it, and the library has ended those requests.
     */
    static void destroyed(Activity activity) {
        for (Iterator<Waiting> waiting = WAITING.iterator(); waiting.hasNext(); ) {
            if (waiting.next().activity == activity) {
                waiting.remove();
            }
        }
        stopped(activity);
    }

    /**
     * {@link FluiActivity#onSaveInstanceState}: the started requests' codes and keys, so that the
     * answers of a process that dies before them still name their key.
     */
    static void save(Bundle state) {
        int[] codes = new int[STARTED.size()];
        String[] keys = new String[codes.length];
        int index = 0;
        for (Map.Entry<Integer, Request> started : STARTED.entrySet()) {
            codes[index] = started.getKey();
            keys[index] = started.getValue().key;
            index++;
        }
        state.putIntArray(SAVED_CODES, codes);
        state.putStringArray(SAVED_KEYS, keys);
        state.putInt(SAVED_NEXT_CODE, nextCode);
    }

    /**
     * {@link FluiActivity#onCreate} with saved state: a started request this process does not
     * know (its process died) is known again, of {@link #EARLIER_PROCESS}, at most
     * {@link #MAX_OPEN}. One the process still knows keeps its epoch.
     */
    static void restore(Bundle state) {
        int[] codes = state.getIntArray(SAVED_CODES);
        String[] keys = state.getStringArray(SAVED_KEYS);
        if (codes == null || keys == null || codes.length != keys.length) {
            return;
        }
        for (int index = 0; index < codes.length && STARTED.size() < MAX_OPEN; index++) {
            if (codes[index] > 0 && keys[index] != null && !STARTED.containsKey(codes[index])) {
                STARTED.put(codes[index], new Request(EARLIER_PROCESS, 0, keys[index]));
            }
        }
        int next = state.getInt(SAVED_NEXT_CODE, 1);
        if (next > nextCode) {
            nextCode = next;
        }
    }

    private static void answer(Answer answer) {
        synchronized (LOCK) {
            ANSWERS.add(answer);
        }
        wake();
    }

    /**
     * Every answer queued, oldest first, as {@code {long[] numbers, String[] keys,
     * Intent[] data}}: {@link #ANSWER_NUMBERS} numbers each (epoch, token, kind, result code), and
     * one key and one {@code Intent}, or {@code null}, each; any thread.
     */
    static Object[] drainAnswers() {
        synchronized (LOCK) {
            int count = ANSWERS.size();
            long[] numbers = new long[count * ANSWER_NUMBERS];
            String[] keys = new String[count];
            Intent[] data = new Intent[count];
            int index = 0;
            for (Answer answer = ANSWERS.poll(); answer != null; answer = ANSWERS.poll()) {
                int at = index * ANSWER_NUMBERS;
                numbers[at] = answer.request.epoch;
                numbers[at + 1] = answer.request.token;
                numbers[at + 2] = answer.kind;
                numbers[at + 3] = answer.resultCode;
                keys[index] = answer.request.key;
                data[index] = answer.data;
                index++;
            }
            return new Object[] {numbers, keys, data};
        }
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
            waiting = !ENTRIES.isEmpty() || !OPENED.isEmpty() || !ANSWERS.isEmpty();
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
