package dev.flui.platform;

import android.app.Activity;
import android.app.UiModeManager;
import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ContentResolver;
import android.content.Context;
import android.database.ContentObserver;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.Point;
import android.graphics.RectF;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.DragEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.CursorAnchorInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * The one view FLUI adds over a NativeActivity's content: the input method's
 * editor, the drop target, the drag source, the receiver of pasted and
 * keyboard content, the stylus handwriting editor, and the host of the
 * semantics tree as virtual views.
 *
 * It draws nothing and takes no touch: FLUI renders to the window surface and
 * reads input from the native input queue. Every method runs on the Java main
 * thread. The native side reaches it only through posted calls, and every
 * native method it calls either queues data for the FLUI owner thread or, for
 * the node provider, answers from the tree the native side applied on this
 * thread, so neither thread ever waits for the other.
 *
 * {@code handle} names the native bridge. The native side clears it, on this
 * thread, before it frees the bridge; a callback after that sees zero and
 * does nothing.
 */
public final class FluiView extends View {
    // Drag event actions, as {@link DragEvent} numbers them, passed to nativeDrag.
    static native boolean nativeDrag(long handle, int action, float x, float y,
            String[] mimeTypes, boolean local, boolean result);

    /** The hovering offer's clip key, or zero when there is no offer. */
    static native long nativeDropOffer(long handle);

    /** Whether the target took the clip already saved under {@code offer}. */
    static native boolean nativeDrop(long handle, long offer, float x, float y, boolean local,
            String[] descriptors);

    static native void nativeImeState(long handle, String text, int selectionStart,
            int selectionEnd, int compositionStart, int compositionEnd, int generation);

    static native void nativeImeAction(long handle, int action, long uptimeMillis);

    static native void nativeImeCursorUpdates(long handle, int mode);

    /** A setting FLUI reads that no configuration change reports has changed. */
    static native void nativeSettingsChanged(long handle);

    /** The window insets this view received have changed. */
    static native void nativeInsetsChanged(long handle);

    /**
     * The keyboard moves from the bottom edge {@code from} to {@code to}, in
     * pixels, starting at {@code startNanos} on {@link System#nanoTime}'s clock
     * for {@code durationMillis}, along an interpolator that takes the values
     * {@code q1}, {@code q2} and {@code q3} a quarter, half and three quarters
     * of the way (API 30).
     */
    static native void nativeKeyboardMotion(long handle, int from, int to, long startNanos,
            long durationMillis, float q1, float q2, float q3);

    /** The keyboard's motion ended or was cancelled (API 30). */
    static native void nativeKeyboardSettled(long handle);

    /** The node info of a virtual view, {@code HOST_VIEW_ID} for this view itself, or null. */
    static native AccessibilityNodeInfo nativeNodeInfo(long handle, View host, int virtualViewId);

    /** The node info of the virtual view that holds the focus of {@code focusType}, or null. */
    static native AccessibilityNodeInfo nativeFindFocus(long handle, View host, int focusType);

    /** Whether the semantics tree took {@code action} on a virtual view. */
    static native boolean nativePerformAction(long handle, View host, int virtualViewId,
            int action, Bundle arguments);

    /**
     * The secure, system and global settings behind FLUI's preferences:
     * configuration changes (locales, font scale and weight, density) reach the
     * native side as the activity's own configuration change instead.
     */
    private static final Uri[] OBSERVED_SETTINGS = {
        Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE),
        // `HAPTIC_FEEDBACK_ENABLED`, deprecated in API 33 but still what the
        // native side reads for the touch-feedback switch.
        Settings.System.getUriFor("haptic_feedback_enabled"),
        Settings.Secure.getUriFor("long_press_timeout"),
        Settings.Secure.getUriFor("high_text_contrast_enabled"),
        // `TIME_12_24`, the 24-hour switch `DateFormat.is24HourFormat` reads.
        Settings.System.getUriFor(Settings.System.TIME_12_24),
    };

    private long handle;
    private final Activity activity;
    private final InputMethodManager inputMethods;

    /** The text the input method edits, a mirror of the focused field's. */
    final SpannableStringBuilder editable = new SpannableStringBuilder();
    /** The owner snapshot {@link #editable} was last set from. */
    int generation;
    private boolean editing;
    private int inputType = EditorInfo.TYPE_CLASS_TEXT;
    private int imeOptions = EditorInfo.IME_ACTION_DONE;
    private boolean handwriting;
    /** Whether this view started the drag the system runs now. */
    private boolean dragging;
    /** Each taken drop's clip and URI permissions, by offer, until FLUI concludes it. */
    private final ConcurrentHashMap<Long, HeldDrop> drops =
            new ConcurrentHashMap<Long, HeldDrop>();
    /** The mirror's state last reported, so an unchanged state is not sent twice. */
    private String reportedText;
    private int reportedSelectionStart = -1;
    private int reportedSelectionEnd = -1;
    private int reportedCompositionStart = -1;
    private int reportedCompositionEnd = -1;
    private int reportedGeneration = -1;
    /** Reports a change of {@link #OBSERVED_SETTINGS}, on this thread. */
    private final ContentObserver settings;
    /** Whether {@link #settings} and the contrast listener are registered. */
    private boolean observing;
    /** The contrast listener on API 34 and above, while {@link #observing}. */
    private Object contrast;
    /** The node provider, while the native side has an accessibility bridge for this view. */
    private AccessibilityNodeProvider nodes;

    private FluiView(Activity activity, long handle) {
        super(activity);
        this.activity = activity;
        this.handle = handle;
        inputMethods = (InputMethodManager) activity.getSystemService(Context.INPUT_METHOD_SERVICE);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setWillNotDraw(true);
        setBackgroundColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= 26) {
            Api26.noFocusHighlight(this);
        }
        if (Build.VERSION.SDK_INT >= 30) {
            FluiKeyboardMotion.install(this);
        }
        if (Build.VERSION.SDK_INT >= 31) {
            FluiContentReceiver.install(this);
        }
        if (Build.VERSION.SDK_INT >= 33) {
            Api33.manualHandwriting(this);
        }
        settings = new ContentObserver(new Handler(Looper.getMainLooper())) {
            @Override
            public void onChange(boolean selfChange) {
                settingsChanged();
            }
        };
    }

    void settingsChanged() {
        long handle = this.handle;
        if (handle != 0) {
            nativeSettingsChanged(handle);
        }
    }

    /**
     * Observe the settings only while in the window: a view that never gets
     * there, or a bridge that freed its handle after failing to keep the view,
     * leaves nothing behind that could call it. Registration comes first, then
     * one report, so a change made before observation began is read too.
     */
    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        if (handle == 0 || observing) {
            return;
        }
        ContentResolver resolver = activity.getContentResolver();
        for (Uri setting : OBSERVED_SETTINGS) {
            resolver.registerContentObserver(setting, false, settings);
        }
        if (Build.VERSION.SDK_INT >= 34) {
            contrast = Api34.observeContrast(this);
        }
        observing = true;
        settingsChanged();
    }

    @Override
    protected void onDetachedFromWindow() {
        stopObserving();
        super.onDetachedFromWindow();
    }

    private void stopObserving() {
        if (!observing) {
            return;
        }
        observing = false;
        activity.getContentResolver().unregisterContentObserver(settings);
        if (contrast != null) {
            Api34.forgetContrast(this, contrast);
            contrast = null;
        }
    }

    /**
     * Create the view and lay it over the activity's whole window. FLUI draws
     * the window's surface and places everything in its pixels, so the view
     * lies in the decor view, not in the content frame, which a window that is
     * not edge to edge sets below the status bar: the input method's caret,
     * the drop point and the semantics tree's bounds are then FLUI's own
     * coordinates. It is the decor view's first child, which an inset change
     * reaches before the content frame can consume it (API 29 and lower).
     */
    public static FluiView install(Activity activity, long handle) {
        FluiView view = new FluiView(activity, handle);
        ViewGroup decor = (ViewGroup) activity.getWindow().getDecorView();
        decor.addView(view, 0, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        return view;
    }

    /** The native bridge is going away: stop calling it and leave the window. */
    public void detach() {
        handle = 0;
        nodes = null;
        setAccessibilityDelegate(null);
        stopObserving();
        editing = false;
        for (HeldDrop drop : drops.values()) {
            drop.release();
        }
        drops.clear();
        inputMethods.restartInput(this);
        ViewGroup parent = (ViewGroup) getParent();
        if (parent != null) {
            parent.removeView(this);
        }
    }

    long handle() {
        return handle;
    }

    /** The view covers the window, so every inset change reaches it. */
    @Override
    public WindowInsets onApplyWindowInsets(WindowInsets insets) {
        long handle = this.handle;
        if (handle != 0) {
            nativeInsetsChanged(handle);
        }
        return super.onApplyWindowInsets(insets);
    }

    // ---------------------------------------------------------------- IME

    @Override
    public boolean onCheckIsTextEditor() {
        return editing;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo info) {
        if (!editing || handle == 0) {
            return null;
        }
        info.inputType = inputType;
        info.imeOptions = imeOptions | EditorInfo.IME_FLAG_NO_FULLSCREEN;
        info.initialSelStart = Selection.getSelectionStart(editable);
        info.initialSelEnd = Selection.getSelectionEnd(editable);
        info.initialCapsMode = 0;
        if (Build.VERSION.SDK_INT >= 30) {
            Api30.surroundingText(info, editable);
        }
        if (Build.VERSION.SDK_INT >= 31) {
            info.contentMimeTypes = FluiContentReceiver.MIME_TYPES;
        }
        if (Build.VERSION.SDK_INT >= 35) {
            Api35.handwriting(info, handwriting);
        }
        return new FluiInputConnection(this);
    }

    /**
     * Focus the editor: the field's text, selection and composition become the
     * mirror the input method edits, and the soft keyboard shows.
     */
    public void focusEditor(int type, int options, boolean stylus, String text, int selectionStart,
            int selectionEnd, int compositionStart, int compositionEnd, int snapshot) {
        inputType = type;
        imeOptions = options;
        handwriting = stylus;
        setMirror(text, selectionStart, selectionEnd, compositionStart, compositionEnd, snapshot);
        editing = true;
        requestFocus();
        inputMethods.restartInput(this);
        inputMethods.showSoftInput(this, 0);
    }

    /** No field takes input any more. */
    public void blurEditor() {
        if (!editing) {
            return;
        }
        editing = false;
        inputMethods.hideSoftInputFromWindow(getWindowToken(), 0);
        inputMethods.restartInput(this);
    }

    /**
     * The field changed outside the input method's edits (or refused one): the
     * mirror becomes the owner's snapshot. A text change restarts the input
     * method, whose own copy of the text is then stale; a selection or
     * composition change only tells it.
     */
    public void pushState(String text, int selectionStart, int selectionEnd, int compositionStart,
            int compositionEnd, int snapshot, boolean restart) {
        setMirror(text, selectionStart, selectionEnd, compositionStart, compositionEnd, snapshot);
        if (!editing) {
            return;
        }
        if (restart) {
            inputMethods.restartInput(this);
        } else {
            inputMethods.updateSelection(this, selectionStart, selectionEnd, compositionStart,
                    compositionEnd);
        }
    }

    private void setMirror(String text, int selectionStart, int selectionEnd, int compositionStart,
            int compositionEnd, int snapshot) {
        editable.clear();
        editable.clearSpans();
        editable.append(text);
        int length = editable.length();
        Selection.setSelection(editable, clamp(selectionStart, length), clamp(selectionEnd, length));
        if (compositionStart >= 0 && compositionEnd >= 0) {
            new Composer(this).setComposingRegion(clamp(compositionStart, length),
                    clamp(compositionEnd, length));
        }
        generation = snapshot;
        reportedText = editable.toString();
        reportedSelectionStart = Selection.getSelectionStart(editable);
        reportedSelectionEnd = Selection.getSelectionEnd(editable);
        reportedCompositionStart = BaseInputConnection.getComposingSpanStart(editable);
        reportedCompositionEnd = BaseInputConnection.getComposingSpanEnd(editable);
        reportedGeneration = snapshot;
    }

    /**
     * Marks a composing range on the mirror: the platform's composing span is
     * private to {@link BaseInputConnection}, so only a connection can place it.
     */
    private static final class Composer extends BaseInputConnection {
        private final FluiView view;

        Composer(FluiView view) {
            super(view, true);
            this.view = view;
        }

        @Override
        public Editable getEditable() {
            return view.editable;
        }
    }

    private static int clamp(int value, int length) {
        return Math.max(0, Math.min(value, length));
    }

    /** Send the mirror's state to the owner, when it changed since the last report. */
    void report() {
        if (handle == 0 || !editing) {
            return;
        }
        String text = editable.toString();
        int selectionStart = Selection.getSelectionStart(editable);
        int selectionEnd = Selection.getSelectionEnd(editable);
        int compositionStart = BaseInputConnection.getComposingSpanStart(editable);
        int compositionEnd = BaseInputConnection.getComposingSpanEnd(editable);
        if (generation == reportedGeneration && text.equals(reportedText)
                && selectionStart == reportedSelectionStart && selectionEnd == reportedSelectionEnd
                && compositionStart == reportedCompositionStart
                && compositionEnd == reportedCompositionEnd) {
            return;
        }
        boolean moved = !text.equals(reportedText) || selectionStart != reportedSelectionStart
                || selectionEnd != reportedSelectionEnd;
        reportedText = text;
        reportedSelectionStart = selectionStart;
        reportedSelectionEnd = selectionEnd;
        reportedCompositionStart = compositionStart;
        reportedCompositionEnd = compositionEnd;
        reportedGeneration = generation;
        nativeImeState(handle, text, selectionStart, selectionEnd, compositionStart,
                compositionEnd, generation);
        // The input method's own edit changed the text or moved the selection: the
        // contract is that the editor says so, and the suggestion and
        // autocorrect-revert state of some keyboards depends on it. A report that
        // only moved the composing region changed neither, and must not be echoed.
        if (moved) {
            inputMethods.updateSelection(this, selectionStart, selectionEnd, compositionStart,
                    compositionEnd);
        }
    }

    void editorAction(int action) {
        if (handle != 0) {
            nativeImeAction(handle, action, android.os.SystemClock.uptimeMillis());
        }
    }

    void cursorUpdates(int mode) {
        if (handle != 0) {
            nativeImeCursorUpdates(handle, mode);
        }
    }

    /**
     * Where the caret, the field and the composing text's characters are, in
     * this view's pixels, for an input method that asked for cursor updates. A
     * negative field width means unknown. {@code characterRects} holds left,
     * top, right and bottom for each UTF-16 index from {@code charactersStart}
     * on, measured on the composing text {@code charactersText}, and
     * {@code characterFlags} each box's visibility. The boxes go to the input
     * method only while the mirror still composes that text there: an edit the
     * owner had not seen when it measured them gets none, not another text's.
     */
    public void updateCursorAnchor(float caretLeft, float caretTop, float caretBottom,
            float fieldLeft, float fieldTop, float fieldRight, float fieldBottom,
            String charactersText, int charactersStart, float[] characterRects,
            int[] characterFlags) {
        if (!editing) {
            return;
        }
        int[] origin = new int[2];
        getLocationOnScreen(origin);
        Matrix matrix = new Matrix();
        matrix.setTranslate(origin[0], origin[1]);
        CursorAnchorInfo.Builder builder = new CursorAnchorInfo.Builder()
                .setMatrix(matrix)
                .setSelectionRange(Selection.getSelectionStart(editable),
                        Selection.getSelectionEnd(editable))
                .setInsertionMarkerLocation(caretLeft, caretTop, caretBottom, caretBottom,
                        CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION);
        int compositionStart = BaseInputConnection.getComposingSpanStart(editable);
        if (compositionStart >= 0) {
            int compositionEnd = BaseInputConnection.getComposingSpanEnd(editable);
            CharSequence composing = editable.subSequence(compositionStart, compositionEnd);
            builder.setComposingText(compositionStart, composing);
            if (charactersStart == compositionStart
                    && charactersText.contentEquals(composing)) {
                for (int unit = 0; unit < characterFlags.length; unit++) {
                    builder.addCharacterBounds(charactersStart + unit, characterRects[unit * 4],
                            characterRects[unit * 4 + 1], characterRects[unit * 4 + 2],
                            characterRects[unit * 4 + 3], characterFlags[unit]);
                }
            }
        }
        if (Build.VERSION.SDK_INT >= 33 && fieldRight >= fieldLeft) {
            Api33.editorBounds(builder, new RectF(fieldLeft, fieldTop, fieldRight, fieldBottom));
        }
        inputMethods.updateCursorAnchorInfo(this, builder.build());
    }

    /**
     * Hand a stylus stroke that began inside the focused field to the input
     * method's handwriting. Returns whether the system can take it.
     */
    public boolean startHandwriting() {
        if (Build.VERSION.SDK_INT < 33 || !editing) {
            return false;
        }
        return Api33.startHandwriting(inputMethods, this);
    }

    /** Insert received text at the mirror's selection, as one input method edit. */
    void receiveText(CharSequence text) {
        int start = Math.max(0, Selection.getSelectionStart(editable));
        int end = Math.max(0, Selection.getSelectionEnd(editable));
        int from = Math.min(start, end);
        int to = Math.max(start, end);
        BaseInputConnection.removeComposingSpans(editable);
        editable.replace(from, to, text);
        Selection.setSelection(editable, from + text.length());
        report();
    }

    boolean isEditing() {
        return editing;
    }

    // ------------------------------------------------------- drag and drop

    @Override
    public boolean onDragEvent(DragEvent event) {
        if (handle == 0) {
            return false;
        }
        int action = event.getAction();
        boolean local = event.getLocalState() != null;
        switch (action) {
            case DragEvent.ACTION_DRAG_STARTED:
            case DragEvent.ACTION_DRAG_ENTERED:
                return nativeDrag(handle, action, event.getX(), event.getY(),
                        mimeTypes(event.getClipDescription()), local, false);
            case DragEvent.ACTION_DRAG_LOCATION:
            case DragEvent.ACTION_DRAG_EXITED:
                return nativeDrag(handle, action, event.getX(), event.getY(), null, local, false);
            case DragEvent.ACTION_DROP:
                return drop(event, local);
            case DragEvent.ACTION_DRAG_ENDED:
                boolean ours = dragging;
                dragging = false;
                return nativeDrag(handle, action, 0, 0, null, ours, event.getResult());
            default:
                return false;
        }
    }

    private static final int DROP_TEXT = 0;
    private static final int DROP_HTML = 1;
    private static final int DROP_URI_LIST = 2;
    private static final int DROP_URI_BYTES = 3;

    private boolean drop(DragEvent event, boolean local) {
        long offer = nativeDropOffer(handle);
        if (offer == 0) {
            return false;
        }
        ClipData clip = event.getClipData();
        Object permissions = null;
        // The grant covers every item without inspecting them at drop time.
        if (clip != null && Build.VERSION.SDK_INT >= 24) {
            permissions = Api24.permissions(activity, event);
        }
        HeldDrop held = new HeldDrop(clip, permissions);
        // Save before nativeDrop wakes the owner: its first request may run on
        // another thread before this Java method returns.
        if (drops.putIfAbsent(offer, held) != null) {
            held.release();
            return false;
        }
        String[] descriptors = mimeTypes(clip == null ? null : clip.getDescription());
        boolean taken = nativeDrop(handle, offer, event.getX(), event.getY(), local, descriptors);
        if (!taken) {
            releaseDrop(offer);
        }
        return taken;
    }

    /** FLUI concluded or refused {@code offer}: release its clip and grant once. */
    public void releaseDrop(long offer) {
        HeldDrop drop = drops.remove(offer);
        if (drop != null) {
            drop.release();
        }
    }

    private static void release(Object permissions) {
        if (permissions != null && Build.VERSION.SDK_INT >= 24) {
            Api24.release(permissions);
        }
    }

    /** A clip readable by background requests, with an idempotent main-thread release. */
    private static final class HeldDrop {
        private ClipData clip;
        private final Object permissions;
        private final AtomicBoolean released = new AtomicBoolean();

        HeldDrop(ClipData clip, Object permissions) {
            this.clip = clip;
            this.permissions = permissions;
        }

        ClipData clip() {
            return released.get() ? null : clip;
        }

        void release() {
            if (released.compareAndSet(false, true)) {
                clip = null;
                FluiView.release(permissions);
            }
        }
    }

    private static String[] mimeTypes(ClipDescription description) {
        if (description == null) {
            return new String[0];
        }
        String[] types = new String[description.getMimeTypeCount()];
        for (int index = 0; index < types.length; index++) {
            types[index] = description.getMimeType(index);
        }
        return types;
    }

    /**
     * Start a system drag of what FLUI offers: plain text and HTML in one item,
     * then one item per URI, described by every media type of the offer
     * (FLUI's session identity among them). Returns whether the system started it.
     */
    public boolean startDrag(String label, String[] mimeTypes, String text, String html,
            String[] uris, int flags, int shadowWidth, int shadowHeight, String session) {
        if (Build.VERSION.SDK_INT < 24) {
            return false;
        }
        ClipData clip = null;
        if (text != null || html != null) {
            CharSequence plain = text != null ? text : "";
            ClipData.Item item = html != null ? new ClipData.Item(plain, html)
                    : new ClipData.Item(plain);
            clip = new ClipData(new ClipDescription(label, mimeTypes), item);
        }
        for (String uri : uris) {
            ClipData.Item item = new ClipData.Item(Uri.parse(uri));
            if (clip == null) {
                clip = new ClipData(new ClipDescription(label, mimeTypes), item);
            } else {
                clip.addItem(item);
            }
        }
        if (clip == null) {
            clip = new ClipData(new ClipDescription(label, mimeTypes), new ClipData.Item(""));
        }
        dragging = Api24.startDrag(this, clip, new Shadow(this, shadowWidth, shadowHeight),
                session, flags);
        return dragging;
    }

    /** A translucent tile the size FLUI asks for, under the touch point. */
    private static final class Shadow extends View.DragShadowBuilder {
        private final int width;
        private final int height;
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);

        Shadow(View view, int width, int height) {
            super(view);
            this.width = Math.max(1, width);
            this.height = Math.max(1, height);
            paint.setColor(Color.argb(96, 128, 128, 128));
        }

        @Override
        public void onProvideShadowMetrics(Point size, Point touch) {
            size.set(width, height);
            touch.set(width / 2, height / 2);
        }

        @Override
        public void onDrawShadow(Canvas canvas) {
            float radius = Math.min(width, height) / 8f;
            canvas.drawRoundRect(new RectF(0, 0, width, height), radius, radius, paint);
        }
    }

    /**
     * One representation of the clip saved under {@code offer}, as UTF-8 for
     * text, HTML and URI lists and as content bytes for a URI. Null means the
     * representation or its source is gone, or that {@code read[0]} crossed
     * {@code limit}; the native side distinguishes the last case. Called on a
     * thread of FLUI's, never while the Java main thread handles the drop.
     */
    public byte[] readDrop(long offer, int source, long limit, long[] read) {
        HeldDrop drop = drops.get(offer);
        ClipData clip = drop == null ? null : drop.clip();
        if (clip == null) {
            return null;
        }
        try {
            switch (source) {
                case DROP_TEXT:
                    return readText(clip, false, limit, read);
                case DROP_HTML:
                    return readText(clip, true, limit, read);
                case DROP_URI_LIST:
                    return readUriList(clip, limit, read);
                case DROP_URI_BYTES:
                    return readUriBytes(clip, limit, read);
                default:
                    return null;
            }
        } catch (Exception error) {
            return null;
        }
    }

    /** Join item text, or read the first HTML value, without a shared builder. */
    private static byte[] readText(ClipData clip, boolean html, long limit, long[] read) {
        BoundedUtf8 out = new BoundedUtf8(limit, read);
        boolean found = false;
        for (int index = 0; index < clip.getItemCount(); index++) {
            ClipData.Item item = clip.getItemAt(index);
            CharSequence text = html ? item.getHtmlText() : item.getText();
            if (text != null) {
                found = true;
                if (!out.write(text) || html) {
                    return out.finished() ? out.bytes() : null;
                }
            }
        }
        return found ? out.bytes() : null;
    }

    private static byte[] readUriList(ClipData clip, long limit, long[] read) {
        BoundedUtf8 out = new BoundedUtf8(limit, read);
        boolean found = false;
        for (int index = 0; index < clip.getItemCount(); index++) {
            Uri uri = clip.getItemAt(index).getUri();
            if (uri != null) {
                found = true;
                if (!out.write(uri.toString()) || !out.write("\r\n")) {
                    return null;
                }
            }
        }
        return found ? out.bytes() : null;
    }

    /** Read the first URI to its end, probing at most one byte past the limit. */
    private byte[] readUriBytes(ClipData clip, long limit, long[] read) {
        Uri uri = null;
        for (int index = 0; index < clip.getItemCount(); index++) {
            uri = clip.getItemAt(index).getUri();
            if (uri != null) {
                break;
            }
        }
        if (uri == null) {
            return null;
        }
        InputStream stream = null;
        try {
            stream = activity.getContentResolver().openInputStream(uri);
            if (stream == null) {
                return null;
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] chunk = new byte[16384];
            while (true) {
                long remaining = limit - read[0];
                int wanted = remaining >= chunk.length
                        ? chunk.length : (int) Math.max(1, remaining + 1);
                int count = stream.read(chunk, 0, wanted);
                if (count < 0) {
                    return out.toByteArray();
                }
                if (count == 0) {
                    return null;
                }
                if (count > remaining) {
                    read[0] = count > Long.MAX_VALUE - read[0]
                            ? Long.MAX_VALUE : read[0] + count;
                    return null;
                }
                read[0] += count;
                out.write(chunk, 0, count);
            }
        } catch (Exception error) {
            return null;
        } finally {
            if (stream != null) {
                try {
                    stream.close();
                } catch (Exception ignored) {
                    // Nothing to recover: the bytes were read or the read failed.
                }
            }
        }
    }

    /** UTF-8 output bounded before allocation, including supplementary scalars. */
    private static final class BoundedUtf8 {
        private final long limit;
        private final long[] read;
        private final ByteArrayOutputStream out = new ByteArrayOutputStream();

        BoundedUtf8(long limit, long[] read) {
            this.limit = limit;
            this.read = read;
        }

        boolean write(CharSequence text) {
            int index = 0;
            while (index < text.length()) {
                char first = text.charAt(index++);
                int scalar = first;
                if (Character.isHighSurrogate(first) && index < text.length()
                        && Character.isLowSurrogate(text.charAt(index))) {
                    scalar = Character.toCodePoint(first, text.charAt(index++));
                } else if (Character.isSurrogate(first)) {
                    scalar = 0xfffd;
                }
                if (!writeScalar(scalar)) {
                    return false;
                }
            }
            return true;
        }

        private boolean writeScalar(int scalar) {
            int width = scalar <= 0x7f ? 1 : scalar <= 0x7ff ? 2 : scalar <= 0xffff ? 3 : 4;
            long total = read[0];
            if (width > limit - total) {
                read[0] = width > Long.MAX_VALUE - total ? Long.MAX_VALUE : total + width;
                return false;
            }
            if (scalar <= 0x7f) {
                out.write(scalar);
            } else if (scalar <= 0x7ff) {
                out.write(0xc0 | (scalar >>> 6));
                out.write(0x80 | (scalar & 0x3f));
            } else if (scalar <= 0xffff) {
                out.write(0xe0 | (scalar >>> 12));
                out.write(0x80 | ((scalar >>> 6) & 0x3f));
                out.write(0x80 | (scalar & 0x3f));
            } else {
                out.write(0xf0 | (scalar >>> 18));
                out.write(0x80 | ((scalar >>> 12) & 0x3f));
                out.write(0x80 | ((scalar >>> 6) & 0x3f));
                out.write(0x80 | (scalar & 0x3f));
            }
            read[0] = total + width;
            return true;
        }

        boolean finished() {
            return read[0] <= limit;
        }

        byte[] bytes() {
            return out.toByteArray();
        }
    }

    // ------------------------------------------------------- accessibility

    /**
     * Install the accessibility delegate or take it out: the native side calls
     * this once the window's accessibility bridge exists, and again when it
     * goes away. A view without one answers as a plain view.
     */
    public void provideNodes(boolean provide) {
        if (provide) {
            nodes = new Nodes();
            setAccessibilityDelegate(new NodesDelegate());
        } else {
            nodes = null;
            setAccessibilityDelegate(null);
        }
    }

    /** Hands the framework {@link #nodes}, while the native bridge lives. */
    private final class NodesDelegate extends View.AccessibilityDelegate {
        @Override
        public AccessibilityNodeProvider getAccessibilityNodeProvider(View host) {
            AccessibilityNodeProvider nodes = FluiView.this.nodes;
            return nodes != null && handle != 0 ? nodes : super.getAccessibilityNodeProvider(host);
        }
    }

    /**
     * FLUI's semantics tree as virtual views of this one. Every call answers
     * from the tree the native side applied on this thread, so none waits for
     * the owner.
     */
    private final class Nodes extends AccessibilityNodeProvider {
        @Override
        public AccessibilityNodeInfo createAccessibilityNodeInfo(int virtualViewId) {
            long handle = FluiView.this.handle;
            return handle == 0 ? null : nativeNodeInfo(handle, FluiView.this, virtualViewId);
        }

        @Override
        public AccessibilityNodeInfo findFocus(int focusType) {
            long handle = FluiView.this.handle;
            return handle == 0 ? null : nativeFindFocus(handle, FluiView.this, focusType);
        }

        @Override
        public boolean performAction(int virtualViewId, int action, Bundle arguments) {
            long handle = FluiView.this.handle;
            return handle != 0
                    && nativePerformAction(handle, FluiView.this, virtualViewId, action, arguments);
        }
    }

    // ------------------------------------------- per-API-level entry points

    private static final class Api24 {
        static boolean startDrag(View view, ClipData clip, View.DragShadowBuilder shadow,
                Object local, int flags) {
            return view.startDragAndDrop(clip, shadow, local, flags);
        }

        static Object permissions(Activity activity, DragEvent event) {
            return activity.requestDragAndDropPermissions(event);
        }

        static void release(Object permissions) {
            ((android.view.DragAndDropPermissions) permissions).release();
        }
    }

    private static final class Api26 {
        static void noFocusHighlight(View view) {
            view.setDefaultFocusHighlightEnabled(false);
        }
    }

    private static final class Api30 {
        static void surroundingText(EditorInfo info, CharSequence text) {
            info.setInitialSurroundingText(text);
        }
    }

    private static final class Api33 {
        static void manualHandwriting(View view) {
            view.setAutoHandwritingEnabled(false);
        }

        static boolean startHandwriting(InputMethodManager inputMethods, View view) {
            inputMethods.startStylusHandwriting(view);
            return true;
        }

        static void editorBounds(CursorAnchorInfo.Builder builder, RectF bounds) {
            builder.setEditorBoundsInfo(new android.view.inputmethod.EditorBoundsInfo.Builder()
                    .setEditorBounds(bounds)
                    .setHandwritingBounds(bounds)
                    .build());
        }
    }

    private static final class Api34 {
        /** Report a change of the system contrast level, on the main thread. */
        static Object observeContrast(final FluiView view) {
            UiModeManager.ContrastChangeListener listener = new UiModeManager.ContrastChangeListener() {
                @Override
                public void onContrastChanged(float contrast) {
                    view.settingsChanged();
                }
            };
            modes(view).addContrastChangeListener(view.activity.getMainExecutor(), listener);
            return listener;
        }

        static void forgetContrast(FluiView view, Object listener) {
            modes(view).removeContrastChangeListener((UiModeManager.ContrastChangeListener) listener);
        }

        private static UiModeManager modes(FluiView view) {
            return (UiModeManager) view.activity.getSystemService(Context.UI_MODE_SERVICE);
        }
    }

    private static final class Api35 {
        static void handwriting(EditorInfo info, boolean enabled) {
            info.setStylusHandwritingEnabled(enabled);
        }
    }
}
