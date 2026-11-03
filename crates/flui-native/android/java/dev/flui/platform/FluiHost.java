package dev.flui.platform;

/**
 * What the native library checks of this dex before it binds a native method
 * (ADR-0255 decision 5).
 *
 * The library loads these classes through the Activity's class loader, from
 * the APK, so the dex and the library are built separately and can come from
 * different FLUI versions. Before {@code RegisterNatives}, the library reads
 * {@link #PROTOCOL} and compares it with its own constant; a mismatch stops
 * the start with an error naming both values, instead of an
 * {@code UnsatisfiedLinkError} or a {@code NoSuchMethodError} far from the
 * cause.
 */
public final class FluiHost {
    /**
     * The version of the contract between these classes and the native
     * library: their native methods and signatures, the methods the library
     * calls, and what each means. Raised with every change to any of them,
     * together with the library's constant.
     */
    public static final int PROTOCOL = 1;

    private FluiHost() {}
}
