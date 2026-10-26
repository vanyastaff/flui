package dev.flui.platform;

import android.content.ClipData;
import android.view.ContentInfo;
import android.view.OnReceiveContentListener;
import android.view.View;

/**
 * {@link FluiView}'s receiver of pasted and keyboard content (API 31).
 *
 * Text reaches the focused field as one input method edit at its selection.
 * What is not text is handed back, which tells the sender it was not taken:
 * FLUI's text store has no place for an image.
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
        StringBuilder text = new StringBuilder();
        java.util.ArrayList<ClipData.Item> rest = new java.util.ArrayList<ClipData.Item>();
        for (int index = 0; index < clip.getItemCount(); index++) {
            ClipData.Item item = clip.getItemAt(index);
            CharSequence itemText = item.getText();
            if (itemText != null) {
                text.append(itemText);
            } else if (item.getUri() == null && item.getIntent() == null) {
                text.append(item.coerceToText(view.getContext()));
            } else {
                rest.add(item);
            }
        }
        if (text.length() > 0) {
            view.receiveText(text);
        }
        if (rest.isEmpty()) {
            return null;
        }
        ClipData remaining = new ClipData(clip.getDescription(), rest.get(0));
        for (int index = 1; index < rest.size(); index++) {
            remaining.addItem(rest.get(index));
        }
        return new ContentInfo.Builder(payload).setClip(remaining).build();
    }
}
