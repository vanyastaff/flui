package dev.flui.system;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.security.SecureRandom;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

/**
 * The documents the application handed to another one (a view, a share), each served
 * read-only under a content URI that names a random token, never a path: {@code
 * content://<package>.flui.documents/<token>/<name>}.
 *
 * <p>The provider is not exported and grants URI permissions, so only an application that
 * received a grant (the activity Android resolved for a view, the share target the user
 * picked) reads a document, and only the documents the application offered ({@link #offer})
 * are served: a URI another application makes up names no token and is refused. A document is
 * the file the library opened and judged, held by its descriptor: what a path names later is
 * never read. The manifest entry comes from {@code flui-system}'s declarations ({@code
 * android.providers}).
 *
 * <p>Android instantiates a content provider by the class name its manifest states, so this
 * is a class of the application's dex; the library calls {@link #offer} and {@link
 * #typeOfName} through JNI.
 */
public final class FluiDocuments extends ContentProvider {
    /** The authority under the application's package. */
    static final String AUTHORITY_SUFFIX = ".flui.documents";

    /** The most offered documents kept; the oldest offer is closed and forgotten past it. */
    static final int MAX_OFFERED = 128;

    private static final String TAG = "FluiDocuments";

    private static final String UNTYPED = "application/octet-stream";

    private static final SecureRandom RANDOM = new SecureRandom();

    /** A document offered: its descriptor, held open, and the name it is served under. */
    private static final class Offered {
        final ParcelFileDescriptor descriptor;
        final String name;

        Offered(ParcelFileDescriptor descriptor, String name) {
            this.descriptor = descriptor;
            this.name = name;
        }
    }

    /** Token to document, oldest first; guarded by itself. */
    private static final LinkedHashMap<String, Offered> OFFERED =
            new LinkedHashMap<String, Offered>(16, 0.75f, false) {
                @Override
                protected boolean removeEldestEntry(Map.Entry<String, Offered> eldest) {
                    if (size() <= MAX_OFFERED) {
                        return false;
                    }
                    close(eldest.getValue().descriptor);
                    return true;
                }
            };

    /**
     * Offer the document open at {@code fd} under {@code name} and answer the URI it is
     * served under, or {@code null} when it could not be kept. The descriptor is duplicated:
     * {@code fd} stays the caller's.
     */
    static Uri offer(Context context, int fd, String name) {
        ParcelFileDescriptor descriptor;
        try {
            descriptor = ParcelFileDescriptor.fromFd(fd);
        } catch (IOException error) {
            Log.w(TAG, "a document could not be kept to hand over", error);
            return null;
        }
        byte[] bytes = new byte[16];
        RANDOM.nextBytes(bytes);
        StringBuilder token = new StringBuilder(32);
        for (byte value : bytes) {
            token.append(String.format(Locale.ROOT, "%02x", value));
        }
        synchronized (OFFERED) {
            OFFERED.put(token.toString(), new Offered(descriptor, name));
        }
        return new Uri.Builder()
                .scheme("content")
                .authority(context.getPackageName() + AUTHORITY_SUFFIX)
                .appendPath(token.toString())
                .appendPath(name)
                .build();
    }

    /** The MIME type of a document named {@code name}, by its extension. */
    static String typeOfName(String name) {
        int dot = name.lastIndexOf('.');
        if (dot >= 0 && dot < name.length() - 1) {
            String type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(
                    name.substring(dot + 1).toLowerCase(Locale.ROOT));
            if (type != null) {
                return type;
            }
        }
        return UNTYPED;
    }

    private static void close(ParcelFileDescriptor descriptor) {
        try {
            descriptor.close();
        } catch (IOException ignored) {
            // Nothing reads it any more.
        }
    }

    private static Offered offered(Uri uri) {
        if (uri.getPathSegments().isEmpty()) {
            return null;
        }
        synchronized (OFFERED) {
            return OFFERED.get(uri.getPathSegments().get(0));
        }
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    /**
     * A descriptor of its own for the reader: the offered file opened again through the
     * descriptor that holds it, so each reader has its own position and none reads what the
     * path names by now.
     */
    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) {
            throw new FileNotFoundException("an offered document is read only");
        }
        Offered document = offered(uri);
        if (document == null) {
            throw new FileNotFoundException("no document is offered under this URI");
        }
        return ParcelFileDescriptor.open(
                new File("/proc/self/fd/" + document.descriptor.getFd()),
                ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs,
            String sortOrder) {
        Offered document = offered(uri);
        if (document == null) {
            return null;
        }
        String[] columns = projection == null
                ? new String[] {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE}
                : projection;
        Object[] row = new Object[columns.length];
        for (int index = 0; index < columns.length; index++) {
            if (OpenableColumns.DISPLAY_NAME.equals(columns[index])) {
                row[index] = document.name;
            } else if (OpenableColumns.SIZE.equals(columns[index])) {
                row[index] = document.descriptor.getStatSize();
            }
        }
        MatrixCursor cursor = new MatrixCursor(columns, 1);
        cursor.addRow(row);
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        Offered document = offered(uri);
        return document == null ? null : typeOfName(document.name);
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        throw new UnsupportedOperationException("offered documents are read only");
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("offered documents are read only");
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException("offered documents are read only");
    }
}
