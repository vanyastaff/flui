package dev.flui.system;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.content.pm.ProviderInfo;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.os.SystemClock;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.security.SecureRandom;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

/**
 * The documents the application handed to another one (a view, a share), each served
 * read-only under a content URI that names a random token, never a path: {@code
 * content://<authority>/<token>/<name>}.
 *
 * <p>The provider is not exported and grants URI permissions, so only an application that
 * received a grant (the activity Android resolved for a view, the share target the user
 * picked) reads a document, and only the documents the application offered ({@link #offer})
 * are served: a URI another application makes up names no token and is refused. A document is
 * the file the library opened and judged, held by its descriptor: what a path names later is
 * never read. The manifest entry comes from {@code flui-system}'s declarations ({@code
 * android.providers}); its authority is whatever the manifest gave it, read from the provider's
 * own {@link ProviderInfo} when Android attaches it, so a build that suffixes the application
 * id serves under the suffixed one.
 *
 * <p>An offer is served for {@link #OFFER_LIFETIME_MS} after it was made and then closed: a
 * grantee that opened the document by then keeps reading its own descriptor, and a later open
 * is refused. At most {@link #MAX_OFFERED} offers are served at once; another is refused
 * ({@link IllegalStateException}) rather than replacing one whose grant may be in use.
 *
 * <p>Android instantiates a content provider by the class name its manifest states, so this
 * is a class of the application's dex; the library calls {@link #authority}, {@link #offer}
 * and {@link #typeOfName} through JNI.
 */
public final class FluiDocuments extends ContentProvider {
    /** The most offers served at once. */
    static final int MAX_OFFERED = 128;

    /** How long an offer is served after it was made, in milliseconds: thirty minutes. */
    static final long OFFER_LIFETIME_MS = 30L * 60L * 1000L;

    private static final String TAG = "FluiDocuments";

    private static final String UNTYPED = "application/octet-stream";

    private static final SecureRandom RANDOM = new SecureRandom();

    /** The authority Android attached the provider under; {@code null} until it did. */
    private static volatile String attachedAuthority;

    /** A document offered: its descriptor, held open, its name and when it was offered. */
    private static final class Offered {
        final ParcelFileDescriptor descriptor;
        final String name;
        final long offeredAt;

        Offered(ParcelFileDescriptor descriptor, String name, long offeredAt) {
            this.descriptor = descriptor;
            this.name = name;
            this.offeredAt = offeredAt;
        }
    }

    /** Token to document, oldest first; guarded by itself. */
    private static final LinkedHashMap<String, Offered> OFFERED =
            new LinkedHashMap<String, Offered>();

    /**
     * The authority the provider serves under, as the manifest gave it; {@code null} when
     * Android never attached the provider (the manifest declares none).
     */
    static String authority() {
        return attachedAuthority;
    }

    /**
     * Offer the document open at {@code fd} under {@code name} and answer the URI it is
     * served under, or {@code null} when the descriptor could not be kept or no authority is
     * attached. The descriptor is duplicated: {@code fd} stays the caller's.
     *
     * @throws IllegalStateException when {@link #MAX_OFFERED} offers are still served
     */
    static Uri offer(Context context, int fd, String name) {
        String authority = attachedAuthority;
        if (authority == null) {
            Log.w(TAG, "the manifest declares no FluiDocuments provider");
            return null;
        }
        byte[] bytes = new byte[16];
        RANDOM.nextBytes(bytes);
        StringBuilder token = new StringBuilder(32);
        for (byte value : bytes) {
            token.append(String.format(Locale.ROOT, "%02x", value));
        }
        synchronized (OFFERED) {
            expire(SystemClock.elapsedRealtime());
            if (OFFERED.size() >= MAX_OFFERED) {
                throw new IllegalStateException("as many documents are offered as are served");
            }
            ParcelFileDescriptor descriptor;
            try {
                descriptor = ParcelFileDescriptor.fromFd(fd);
            } catch (IOException error) {
                Log.w(TAG, "a document could not be kept to hand over", error);
                return null;
            }
            OFFERED.put(
                    token.toString(),
                    new Offered(descriptor, name, SystemClock.elapsedRealtime()));
        }
        return new Uri.Builder()
                .scheme("content")
                .authority(authority)
                .appendPath(token.toString())
                .appendPath(name)
                .build();
    }

    /** Close and forget the offers older than {@link #OFFER_LIFETIME_MS}; under the lock. */
    private static void expire(long now) {
        Iterator<Map.Entry<String, Offered>> offers = OFFERED.entrySet().iterator();
        while (offers.hasNext()) {
            Offered offered = offers.next().getValue();
            if (now - offered.offeredAt < OFFER_LIFETIME_MS) {
                // Oldest first: the rest are younger.
                return;
            }
            close(offered.descriptor);
            offers.remove();
        }
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
            expire(SystemClock.elapsedRealtime());
            return OFFERED.get(uri.getPathSegments().get(0));
        }
    }

    /**
     * Keep the authority the manifest gave this provider: the first of its {@code ;}-separated
     * authorities, which is the one {@code flui build} writes.
     */
    @Override
    public void attachInfo(Context context, ProviderInfo info) {
        super.attachInfo(context, info);
        if (info != null && info.authority != null) {
            attachedAuthority = info.authority.split(";")[0];
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
