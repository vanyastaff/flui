package dev.flui.platform;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.os.Build;
import android.text.Editable;
import android.text.Selection;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;

/**
 * The input method's connection to {@link FluiView}'s mirror.
 *
 * Every read is answered from the mirror at once, on the Java main thread,
 * without asking FLUI's owner thread. Every edit applies to the mirror, so the
 * next read sees it, and the mirror's new state goes to the owner when the
 * edit (or the batch it belongs to) ends; the owner replays it into the
 * field's text store in order.
 */
final class FluiInputConnection extends BaseInputConnection {
    private final FluiView view;
    private int batch;

    FluiInputConnection(FluiView view) {
        super(view, true);
        this.view = view;
    }

    @Override
    public Editable getEditable() {
        return view.editable;
    }

    private boolean edited(boolean result) {
        if (batch == 0) {
            view.report();
        }
        return result;
    }

    @Override
    public boolean beginBatchEdit() {
        batch++;
        return true;
    }

    @Override
    public boolean endBatchEdit() {
        if (batch > 0) {
            batch--;
            if (batch == 0) {
                view.report();
            }
        }
        return batch > 0;
    }

    @Override
    public void closeConnection() {
        super.closeConnection();
        if (batch > 0) {
            batch = 0;
            view.report();
        }
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        return edited(super.commitText(text, newCursorPosition));
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        return edited(super.setComposingText(text, newCursorPosition));
    }

    @Override
    public boolean setComposingRegion(int start, int end) {
        return edited(super.setComposingRegion(start, end));
    }

    @Override
    public boolean finishComposingText() {
        return edited(super.finishComposingText());
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        return edited(super.deleteSurroundingText(beforeLength, afterLength));
    }

    @Override
    public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
        return edited(super.deleteSurroundingTextInCodePoints(beforeLength, afterLength));
    }

    @Override
    public boolean setSelection(int start, int end) {
        return edited(super.setSelection(start, end));
    }

    @Override
    public boolean performEditorAction(int action) {
        view.editorAction(action);
        return true;
    }

    @Override
    public boolean requestCursorUpdates(int mode) {
        view.cursorUpdates(mode);
        return true;
    }

    @Override
    public boolean performContextMenuAction(int id) {
        Editable text = getEditable();
        switch (id) {
            case android.R.id.selectAll:
                Selection.setSelection(text, 0, text.length());
                return edited(true);
            case android.R.id.copy:
                return copy(false);
            case android.R.id.cut:
                return copy(true);
            case android.R.id.paste:
            case android.R.id.pasteAsPlainText:
                paste();
                return true;
            default:
                return false;
        }
    }

    /**
     * Copy the selected text to the clipboard and, for a cut, delete it from
     * the mirror as one input method edit. A protected field refuses both:
     * its mirror holds bullets, and its text never leaves it. An empty
     * selection copies nothing.
     */
    private boolean copy(boolean cut) {
        if (view.isProtected()) {
            return false;
        }
        Editable text = getEditable();
        int start = Selection.getSelectionStart(text);
        int end = Selection.getSelectionEnd(text);
        int from = Math.max(0, Math.min(start, end));
        int to = Math.min(text.length(), Math.max(start, end));
        if (from >= to) {
            return true;
        }
        ClipboardManager clipboard = (ClipboardManager) view.getContext()
                .getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard == null) {
            return false;
        }
        clipboard.setPrimaryClip(ClipData.newPlainText(null, text.subSequence(from, to)));
        if (!cut) {
            return true;
        }
        BaseInputConnection.removeComposingSpans(text);
        text.delete(from, to);
        Selection.setSelection(text, from);
        return edited(true);
    }

    /**
     * Paste the clipboard into the mirror: through the view's content
     * receiver where the platform has one (API 31), before it as the text
     * {@link FluiView#clipText} admits.
     */
    private void paste() {
        ClipboardManager clipboard = (ClipboardManager) view.getContext()
                .getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData clip = clipboard == null ? null : clipboard.getPrimaryClip();
        if (clip == null) {
            return;
        }
        if (Build.VERSION.SDK_INT >= 31) {
            FluiContentReceiver.paste(view, clip);
            return;
        }
        CharSequence text = FluiView.clipText(clip);
        if (text != null) {
            view.receiveText(text);
        }
    }

    @Override
    public ExtractedText getExtractedText(ExtractedTextRequest request, int flags) {
        return view.extractedText(this, request, flags);
    }
}
