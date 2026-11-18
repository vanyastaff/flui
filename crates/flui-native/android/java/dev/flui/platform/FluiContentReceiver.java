package dev.flui.platform;

import android.content.ClipData;
import android.view.ContentInfo;
import android.view.OnReceiveContentListener;
import android.view.View;

/**
 * {@link FluiView}'s receiver of pasted and keyboard content (API 31).
 *
 * Text reaches the focused field as one input method edit at its selection,
 * within the size {@link FluiView#clipText} admits. What is not text, and
 * text past that size, is handed back, which tells the sender it was not
 * taken: FLUI's text store has no place for an image, and no provider's
 * content is read here.
 *
 * Its own class, loaded only on API 31 and later: the interface it implements
 * does not exist before.
 */
final class FluiContentReceiver implements OnReceiveContentListener {
    /** What the receiver takes, also announced to input methods. */
    static final String[] MIME_TYPES = {"text/*"};

    static void install(FluiView view) {
        view.setOnReceiveContentListener(MIME_TYPES, new FluiContentReceiver());
    }

    static void paste(FluiView view, ClipData clip) {
        view.performReceiveContent(new ContentInfo.Builder(clip, ContentInfo.SOURCE_CLIPBOARD)
                .build());
    }

    @Override
    public ContentInfo onReceiveContent(View target, ContentInfo payload) {
        FluiView view = (FluiView) target;
        if (view.handle() == 0 || !view.isEditing()) {
            return payload;
        }
        ClipData clip = payload.getClip();
        CharSequence text = FluiView.clipText(clip);
        if (text != null) {
            view.receiveText(text);
        }
        // What holds no text, and text refused for its size, is not taken.
        java.util.ArrayList<ClipData.Item> rest = new java.util.ArrayList<ClipData.Item>();
        for (int index = 0; index < clip.getItemCount(); index++) {
            ClipData.Item item = clip.getItemAt(index);
            if (text == null || item.getText() == null) {
                rest.add(item);
            }
        }
        if (rest.isEmpty()) {
            return null;
        }
        if (rest.size() == clip.getItemCount()) {
            return payload;
        }
        ClipData remaining = new ClipData(clip.getDescription(), rest.get(0));
        for (int index = 1; index < rest.size(); index++) {
            remaining.addItem(rest.get(index));
        }
        return new ContentInfo.Builder(payload).setClip(remaining).build();
    }
}
