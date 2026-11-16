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
import java.util.HashSet;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;

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
 * are pairs of kind and value ({@link #peekEntries}); an intent is a record of its
 * launch, action and URIs ({@link #peekOpened}); an answer to an activity
 * the library started for a result names the request's epoch, token and key
 * ({@link #peekAnswers}, {@link #commitAnswers}). The open requests are saved with the Activity's
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

    /** An answer: Android refused to start the request's activity. */
    static final int ANSWER_NOT_LAUNCHED = 2;

    /** An answer: the host held as many requests as it holds, {@link #MAX_OPEN}. */
    static final int ANSWER_TOO_MANY = 3;

    /** The numbers of one answer in {@link #peekAnswers}: epoch, token, kind, result code. */
    static final int ANSWER_NUMBERS = 4;

    /** The epoch of a request restored from the saved state of a process that died. */
    static final long EARLIER_PROCESS = 0;

    /**
     * The most requests open at once across the platforms of the process: waiting to start,
     * started, and answered but not drained; the library's {@code MAX_OPEN_REQUESTS}. A refusal
     * past it is an answer too, so at most twice as many answers wait.
     */
    static final int MAX_OPEN = 16;

    /**
     * The most of {@link #MAX_OPEN} one capability holds across the platforms of the process,
     * its earlier Activities' requests that are still to come included, so one capability cannot
     * keep the others from asking; the library's {@code MAX_OPEN_REQUESTS_PER_CAPABILITY}.
     */
    static final int MAX_OPEN_PER_CAPABILITY = 8;

    /**
     * The first of the request codes the host keeps for itself (see {@link FluiActivity}):
     * within the 16 bits {@code FragmentActivity} allows, so an application's own codes below
     * it never meet the host's.
     */
    static final int FIRST_HOST_CODE = 0xFF00;

    /** The last of the request codes the host keeps for itself. */
    static final int LAST_HOST_CODE = 0xFFFF;

    private static final String SAVED_CODES = "dev.flui.platform.requests.codes";

    private static final String SAVED_CAPABILITIES = "dev.flui.platform.requests.capabilities";

    private static final String SAVED_KEYS = "dev.flui.platform.requests.keys";

    private static final String SAVED_NEXT_CODE = "dev.flui.platform.requests.next";

    /**
     * A request the library took: the epoch of its platform ({@link #nextEpoch}), the token the
     * library minted, and the key that names its purpose across Activities: the capability's
     * name and the provider's key, kept and compared apart.
     */
    private static final class Request {
        final long epoch;
        final long token;
        final String capability;
        final String key;

        Request(long epoch, long token, String capability, String key) {
            this.epoch = epoch;
            this.token = token;
            this.capability = capability;
            this.key = key;
        }

        boolean is(long epoch, long token) {
            return this.epoch == epoch && this.token == token;
        }

        boolean isEarlierUnder(long epoch, String capability, String key) {
            return this.epoch != epoch && this.capability.equals(capability) && this.key.equals(key);
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

    /** Started requests by their request code. Guarded by {@link #LOCK}. */
    private static final HashMap<Integer, Request> STARTED = new HashMap<>();

    /**
     * The codes of started requests the library cancelled, until Android answers for them: not
     * counted as open, not reused. Guarded by {@link #LOCK}.
     */
    private static final HashSet<Integer> CANCELLED = new HashSet<>();

    /** Requests taken while their Activity was stopped, oldest first. Guarded by {@link #LOCK}. */
    private static final ArrayDeque<Waiting> WAITING = new ArrayDeque<>();

    /** The next request code to try. Guarded by {@link #LOCK}. */
    private static int nextCode = FIRST_HOST_CODE;

    /** The Activity between its {@code onStart} and {@code onStop}. Guarded by {@link #LOCK}. */
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

    /**
     * Every intent's record queued, oldest first; any thread. They stay
     * queued until {@link #commitOpened}, so a read that fails loses none.
     */
    static String[][] peekOpened() {
        synchronized (LOCK) {
            return OPENED.toArray(new String[0][]);
        }
    }

    /** The library holds the oldest {@code count} records: they leave the queue; any thread. */
    static void commitOpened(int count) {
        synchronized (LOCK) {
            for (int index = 0; index < count && !OPENED.isEmpty(); index++) {
                OPENED.poll();
            }
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
     * the library's {@code token} under {@code capability}'s {@code key}, of the platform whose
     * epoch is {@code epoch}. While {@code activity} is stopped it waits for {@link #started}.
     * Past {@link #MAX_OPEN} open requests, or {@link #MAX_OPEN_PER_CAPABILITY} of {@code
     * capability}'s, it is answered with {@link #ANSWER_TOO_MANY}, and when Android refuses with
     * {@link #ANSWER_NOT_LAUNCHED}.
     */
    static void launch(
            Activity activity, Intent intent, long epoch, long token, String capability, String key) {
        Request request = new Request(epoch, token, capability, key);
        synchronized (LOCK) {
            if (STARTED.size() + WAITING.size() + ANSWERS.size() >= MAX_OPEN
                    || openUnder(capability) >= MAX_OPEN_PER_CAPABILITY) {
                Log.w(TAG, "as many activity requests are open as FLUI holds; this one is refused");
                ANSWERS.add(new Answer(request, ANSWER_TOO_MANY, 0, null));
                request = null;
            } else if (activity != startedActivity) {
                WAITING.add(new Waiting(activity, intent, request));
                request = null;
            }
        }
        if (request == null) {
            wake();
            return;
        }
        start(activity, intent, request);
    }

    private static void start(Activity activity, Intent intent, Request request) {
        int code;
        synchronized (LOCK) {
            code = freeCode(STARTED, CANCELLED, nextCode);
            if (code > 0) {
                nextCode = code == LAST_HOST_CODE ? FIRST_HOST_CODE : code + 1;
                STARTED.put(code, request);
            }
        }
        if (code <= 0) {
            answer(new Answer(request, ANSWER_TOO_MANY, 0, null));
            return;
        }
        try {
            activity.startActivityForResult(intent, code);
        } catch (RuntimeException error) {
            // `ActivityNotFoundException`, `SecurityException`: nothing started.
            synchronized (LOCK) {
                STARTED.remove(code);
            }
            Log.w(TAG, "an activity FLUI was asked to start did not start", error);
            answer(new Answer(request, ANSWER_NOT_LAUNCHED, 0, null));
        }
    }

    /**
     * The first request code from {@code next} on, through {@link #LAST_HOST_CODE} and then from
     * {@link #FIRST_HOST_CODE}, that neither {@code started} nor {@code cancelled} holds; {@code 0}
     * when every code of the host's range is held. A code only tells Android's answer apart while
     * its request is started, or its cancelled activity has not answered yet; the library matches
     * the answer by epoch and token.
     */
    static int freeCode(Map<Integer, ?> started, Set<Integer> cancelled, int next) {
        int first = next < FIRST_HOST_CODE || next > LAST_HOST_CODE ? FIRST_HOST_CODE : next;
        int count = LAST_HOST_CODE - FIRST_HOST_CODE + 1;
        for (int step = 0; step < count; step++) {
            int code = FIRST_HOST_CODE + (first - FIRST_HOST_CODE + step) % count;
            if (!started.containsKey(code) && !cancelled.contains(code)) {
                return code;
            }
        }
        return 0;
    }

    /** Whether {@code code} is one of the request codes the host keeps for itself. */
    static boolean isHostCode(int code) {
        return code >= FIRST_HOST_CODE && code <= LAST_HOST_CODE;
    }

    /**
     * {@link FluiActivity#onActivityResult}: the answer of a request FLUI started waits for the
     * owner. A code outside the host's range is the application's own, and is not looked at.
     * {@code data} comes from another application: it is held as it is, and only the provider
     * that asked reads it.
     */
    static void activityResult(int code, int resultCode, Intent data) {
        if (!isHostCode(code)) {
            return;
        }
        Request request;
        synchronized (LOCK) {
            // A cancelled request's answer frees its code and reaches nobody.
            request = CANCELLED.remove(code) ? null : STARTED.remove(code);
        }
        if (request != null) {
            answer(new Answer(request, ANSWER_RESULT, resultCode, data));
        }
    }

    /**
     * The library cancelled its request {@code token} of the platform {@code epoch}; Java main
     * thread. One still waiting to start is forgotten; a started one is finished and leaves the
     * open requests at once, so its place counts no more: its code is parked in {@link
     * #CANCELLED} until Android's answer for it comes, which is then dropped, so the code is not
     * reused before.
     */
    static void cancel(Activity activity, long epoch, long token) {
        int finish = 0;
        synchronized (LOCK) {
            for (Iterator<Waiting> waiting = WAITING.iterator(); waiting.hasNext(); ) {
                if (waiting.next().request.is(epoch, token)) {
                    waiting.remove();
                    return;
                }
            }
            for (Map.Entry<Integer, Request> started : STARTED.entrySet()) {
                if (started.getValue().is(epoch, token)) {
                    finish = started.getKey();
                    break;
                }
            }
            if (finish != 0) {
                STARTED.remove(finish);
                CANCELLED.add(finish);
            }
        }
        if (finish != 0) {
            activity.finishActivity(finish);
        }
    }

    /**
     * How many answers are still to come under {@code capability}'s {@code key} for platforms
     * other than {@code epoch}: requests waiting to start, started, or answered and not drained,
     * whatever the answer; any thread. This is the one definition the library holds its
     * reattachments to: at most this many wait under the key. {@link FluiActivity} restores the
     * saved requests before the library starts, so none is missed here.
     */
    static int stillToCome(long epoch, String capability, String key) {
        synchronized (LOCK) {
            int count = 0;
            for (Waiting waiting : WAITING) {
                count += waiting.request.isEarlierUnder(epoch, capability, key) ? 1 : 0;
            }
            for (Request request : STARTED.values()) {
                count += request.isEarlierUnder(epoch, capability, key) ? 1 : 0;
            }
            for (Answer answer : ANSWERS) {
                count += answer.request.isEarlierUnder(epoch, capability, key) ? 1 : 0;
            }
            return count;
        }
    }

    /** How many requests are open under {@code capability}, of every platform. Under {@link #LOCK}. */
    private static int openUnder(String capability) {
        int count = 0;
        for (Request request : STARTED.values()) {
            count += request.capability.equals(capability) ? 1 : 0;
        }
        for (Waiting waiting : WAITING) {
            count += waiting.request.capability.equals(capability) ? 1 : 0;
        }
        for (Answer answer : ANSWERS) {
            count += answer.request.capability.equals(capability) ? 1 : 0;
        }
        return count;
    }

    /** {@link FluiActivity#onStart}: the requests waiting for {@code activity} start. */
    static void started(Activity activity) {
        ArrayList<Waiting> ready = new ArrayList<>();
        synchronized (LOCK) {
            startedActivity = activity;
            for (Iterator<Waiting> waiting = WAITING.iterator(); waiting.hasNext(); ) {
                Waiting next = waiting.next();
                if (next.activity == activity) {
                    waiting.remove();
                    ready.add(next);
                }
            }
        }
        for (Waiting waiting : ready) {
            start(activity, waiting.intent, waiting.request);
        }
    }

    /** {@link FluiActivity#onStop}. */
    static void stopped(Activity activity) {
        synchronized (LOCK) {
            if (startedActivity == activity) {
                startedActivity = null;
            }
        }
    }

    /**
     * {@link FluiActivity#onDestroy}: what waits to start from {@code activity} never will. Its
     * platform ends with it, and the library has ended those requests.
     */
    static void destroyed(Activity activity) {
        synchronized (LOCK) {
            for (Iterator<Waiting> waiting = WAITING.iterator(); waiting.hasNext(); ) {
                if (waiting.next().activity == activity) {
                    waiting.remove();
                }
            }
            if (startedActivity == activity) {
                startedActivity = null;
            }
        }
    }

    /**
     * {@link FluiActivity#onSaveInstanceState}: the started requests' codes and keys, so that the
     * answers of a process that dies before them still name their key.
     */
    static void save(Bundle state) {
        int[] codes;
        String[] capabilities;
        String[] keys;
        int next;
        synchronized (LOCK) {
            codes = new int[STARTED.size()];
            capabilities = new String[codes.length];
            keys = new String[codes.length];
            int index = 0;
            for (Map.Entry<Integer, Request> started : STARTED.entrySet()) {
                codes[index] = started.getKey();
                capabilities[index] = started.getValue().capability;
                keys[index] = started.getValue().key;
                index++;
            }
            next = nextCode;
        }
        state.putIntArray(SAVED_CODES, codes);
        state.putStringArray(SAVED_CAPABILITIES, capabilities);
        state.putStringArray(SAVED_KEYS, keys);
        state.putInt(SAVED_NEXT_CODE, next);
    }

    /**
     * {@link FluiActivity#onCreate} with saved state: a started request this process does not
     * know (its process died) is known again, of {@link #EARLIER_PROCESS}, at most
     * {@link #MAX_OPEN}; a code outside the host's range is not taken. One the process still
     * knows keeps its epoch.
     */
    static void restore(Bundle state) {
        int[] codes = state.getIntArray(SAVED_CODES);
        String[] capabilities = state.getStringArray(SAVED_CAPABILITIES);
        String[] keys = state.getStringArray(SAVED_KEYS);
        if (codes == null
                || capabilities == null
                || keys == null
                || codes.length != keys.length
                || codes.length != capabilities.length) {
            return;
        }
        synchronized (LOCK) {
            for (int index = 0; index < codes.length && STARTED.size() < MAX_OPEN; index++) {
                int code = codes[index];
                if (isHostCode(code)
                        && capabilities[index] != null
                        && keys[index] != null
                        && !STARTED.containsKey(code)) {
                    STARTED.put(
                            code,
                            new Request(EARLIER_PROCESS, 0, capabilities[index], keys[index]));
                }
            }
            int next = state.getInt(SAVED_NEXT_CODE, FIRST_HOST_CODE);
            if (isHostCode(next)) {
                nextCode = next;
            }
        }
    }

    private static void answer(Answer answer) {
        synchronized (LOCK) {
            ANSWERS.add(answer);
        }
        wake();
    }

    /**
     * The oldest answers queued, at most {@code max}, oldest first, as
     * {@code {long[] numbers, String[] capabilities, String[] keys, Intent[] data}}:
     * {@link #ANSWER_NUMBERS} numbers each (epoch, token, kind, result code), and one capability,
     * one key and one {@code Intent}, or {@code null}, each; any thread. They stay queued until
     * {@link #commitAnswers}, so a read that fails loses none.
     */
    static Object[] peekAnswers(int max) {
        synchronized (LOCK) {
            int count = Math.min(Math.max(max, 0), ANSWERS.size());
            long[] numbers = new long[count * ANSWER_NUMBERS];
            String[] capabilities = new String[count];
            String[] keys = new String[count];
            Intent[] data = new Intent[count];
            int index = 0;
            for (Answer answer : ANSWERS) {
                if (index == count) {
                    break;
                }
                int at = index * ANSWER_NUMBERS;
                numbers[at] = answer.request.epoch;
                numbers[at + 1] = answer.request.token;
                numbers[at + 2] = answer.kind;
                numbers[at + 3] = answer.resultCode;
                capabilities[index] = answer.request.capability;
                keys[index] = answer.request.key;
                data[index] = answer.data;
                index++;
            }
            return new Object[] {numbers, capabilities, keys, data};
        }
    }

    /**
     * The library holds the oldest {@code count} answers {@link #peekAnswers} returned: they
     * leave the queue; any thread. Answers how many are still queued.
     */
    static int commitAnswers(int count) {
        synchronized (LOCK) {
            for (int index = 0; index < count && !ANSWERS.isEmpty(); index++) {
                ANSWERS.poll();
            }
            return ANSWERS.size();
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

    /**
     * Every entry queued, oldest first, as kind and value pairs; any thread.
     * They stay queued until {@link #commitEntries}, so a read that fails
     * loses none.
     */
    static int[] peekEntries() {
        synchronized (LOCK) {
            int[] pairs = new int[ENTRIES.size() * 2];
            int index = 0;
            for (int[] entry : ENTRIES) {
                pairs[index++] = entry[0];
                pairs[index++] = entry[1];
            }
            return pairs;
        }
    }

    /** The library holds the oldest {@code count} entries: they leave the queue; any thread. */
    static void commitEntries(int count) {
        synchronized (LOCK) {
            for (int index = 0; index < count && !ENTRIES.isEmpty(); index++) {
                ENTRIES.poll();
            }
        }
    }

    private static native void nativeWake(long handle);
}
