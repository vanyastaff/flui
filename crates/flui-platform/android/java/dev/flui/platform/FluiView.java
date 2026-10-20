package dev.flui.platform;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipDescription;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.Point;
import android.graphics.RectF;
import android.net.Uri;
import android.os.Build;
import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.DragEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.CursorAnchorInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.util.HashMap;

/**
 * The one view FLUI adds over a NativeActivity's content: the input method's
 * editor, the drop target, the drag source, the receiver of pasted and
 * keyboard content, and the stylus handwriting editor.
 *
 * It draws nothing and takes no touch: FLUI renders to the window surface and
 * reads input from the native input queue. Every method runs on the Java main
 * thread. The native side reaches it only through posted calls, and every
 * native method it calls only queues data for the FLUI owner thread, so
 * neither thread ever waits for the other.
 *
 * {@code handle} names the native bridge. The native side clears it, on this
 * thread, before it frees the bridge; a callback after that sees zero and
 * does nothing.
 */
public final class FluiView extends View {
    // Drag event actions, as {@link DragEvent} numbers them, passed to nativeDrag.
    static native boolean nativeDrag(long handle, int action, float x, float y,
            String[] mimeTypes, boolean local, boolean result);

    /** The offer a taken drop holds until FLUI concludes it, or zero when it is refused. */
    static native long nativeDrop(long handle, float x, float y, boolean local, String text,
            String html, String[] uris);

    static native void nativeImeState(long handle, String text, int selectionStart,
            int selectionEnd, int compositionStart, int compositionEnd, int generation);

    static native void nativeImeAction(long handle, int action, long uptimeMillis);

    static native void nativeImeCursorUpdates(long handle, int mode);

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
    /** The permissions of each taken drop's content URIs, by its offer, until FLUI concludes it. */
    private final HashMap<Long, Object> dropPermissions = new HashMap<Long, Object>();
    /** The mirror's state last reported, so an unchanged state is not sent twice. */
    private String reportedText;
    private int reportedSelectionStart = -1;
    private int reportedSelectionEnd = -1;
    private int reportedCompositionStart = -1;
    private int reportedCompositionEnd = -1;
    private int reportedGeneration = -1;

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
        if (Build.VERSION.SDK_INT >= 31) {
            FluiContentReceiver.install(this);
        }
        if (Build.VERSION.SDK_INT >= 33) {
            Api33.manualHandwriting(this);
        }
    }

    /** Create the view and lay it over the activity's whole content. */
    public static FluiView install(Activity activity, long handle) {
        FluiView view = new FluiView(activity, handle);
        activity.addContentView(view, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        return view;
    }

    /** The native bridge is going away: stop calling it and leave the window. */
    public void detach() {
        handle = 0;
        editing = false;
        for (Object permissions : dropPermissions.values()) {
            release(permissions);
        }
        dropPermissions.clear();
        inputMethods.restartInput(this);
        ViewGroup parent = (ViewGroup) getParent();
        if (parent != null) {
            parent.removeView(this);
        }
    }

    long handle() {
        return handle;
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
        reportedText = text;
        reportedSelectionStart = selectionStart;
        reportedSelectionEnd = selectionEnd;
        reportedCompositionStart = compositionStart;
        reportedCompositionEnd = compositionEnd;
        reportedGeneration = generation;
        nativeImeState(handle, text, selectionStart, selectionEnd, compositionStart,
                compositionEnd, generation);
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
     * Where the caret and the field are, in this view's pixels, for an input
     * method that asked for cursor updates. A negative width means unknown.
     */
    public void updateCursorAnchor(float caretLeft, float caretTop, float caretBottom,
            float fieldLeft, float fieldTop, float fieldRight, float fieldBottom) {
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
            builder.setComposingText(compositionStart,
                    editable.subSequence(compositionStart, compositionEnd));
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

    private boolean drop(DragEvent event, boolean local) {
        ClipData clip = event.getClipData();
        String text = null;
        String html = null;
        String[] uris = new String[0];
        Object permissions = null;
        if (clip != null) {
            StringBuilder plain = new StringBuilder();
            java.util.ArrayList<String> found = new java.util.ArrayList<String>();
            for (int index = 0; index < clip.getItemCount(); index++) {
                ClipData.Item item = clip.getItemAt(index);
                CharSequence itemText = item.getText();
                if (itemText != null) {
                    plain.append(itemText);
                }
                if (html == null && item.getHtmlText() != null) {
                    html = item.getHtmlText();
                }
                if (item.getUri() != null) {
                    found.add(item.getUri().toString());
                }
            }
            if (plain.length() > 0) {
                text = plain.toString();
            }
            uris = found.toArray(new String[0]);
            // Asked for before the native side answers: once it takes the drop,
            // FLUI may read a URI before this method returns.
            if (uris.length > 0 && Build.VERSION.SDK_INT >= 24) {
                permissions = Api24.permissions(activity, event);
            }
        }
        long offer = nativeDrop(handle, event.getX(), event.getY(), local, text, html, uris);
        if (offer == 0) {
            release(permissions);
        } else if (permissions != null) {
            dropPermissions.put(offer, permissions);
        }
        return offer != 0;
    }

    /** FLUI concluded the drop of {@code offer}: give back the URI permissions it held. */
    public void releaseDrop(long offer) {
        release(dropPermissions.remove(offer));
    }

    private static void release(Object permissions) {
        if (permissions != null && Build.VERSION.SDK_INT >= 24) {
            Api24.release(permissions);
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
     * The bytes behind a content URI of a drop, at most {@code limit} of them;
     * null when it cannot be read or is larger. {@code read[0]} is how many
     * bytes it read, past {@code limit} when it stopped there. Called on a
     * thread of FLUI's.
     */
    public byte[] readUri(String uri, long limit, long[] read) {
        InputStream stream = null;
        try {
            stream = activity.getContentResolver().openInputStream(Uri.parse(uri));
            if (stream == null) {
                return null;
            }
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] chunk = new byte[16384];
            int count;
            while ((count = stream.read(chunk)) > 0) {
                read[0] += count;
                if (read[0] > limit) {
                    return null;
                }
                out.write(chunk, 0, count);
            }
            return out.toByteArray();
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

    private static final class Api35 {
        static void handwriting(EditorInfo info, boolean enabled) {
            info.setStylusHandwritingEnabled(enabled);
        }
    }
}
