package dev.flui.system;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;

/**
 * The one object Android calls {@code flui-system}'s native code back through: the handler
 * of a {@code java.lang.reflect.Proxy}, so a {@code Runnable} posted to the main thread and a
 * dialog's listeners need no class of their own.
 *
 * <p>The library makes every instance, through JNI, with a handle of its own, and binds
 * {@link #called} and {@link #released} after it has read {@link #PROTOCOL}. A call of an
 * interface method reaches {@link #called} with the method's name; the methods every object
 * has are answered here, since a proxy routes those to its handler too. The handle is the
 * library's until this object is collected, which {@link #released} tells it.
 */
public final class FluiCallback implements InvocationHandler {
    /** The contract with the native library; raised with every change of a signature here. */
    public static final int PROTOCOL = 1;

    private final long handle;

    FluiCallback(long handle) {
        this.handle = handle;
    }

    @Override
    public Object invoke(Object proxy, Method method, Object[] args) {
        if (method.getDeclaringClass() == Object.class) {
            switch (method.getName()) {
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    return "FluiCallback@" + Long.toHexString(handle);
            }
        }
        // The one argument the library reads: the button of `OnClickListener.onClick`.
        int argument = 0;
        if (args != null && args.length > 1 && args[1] instanceof Integer) {
            argument = (Integer) args[1];
        }
        called(handle, method.getName(), argument);
        return null;
    }

    @Override
    protected void finalize() throws Throwable {
        try {
            released(handle);
        } finally {
            super.finalize();
        }
    }

    /** The interface method {@code method} was called on the proxy of {@code handle}. */
    private static native void called(long handle, String method, int argument);

    /** Nothing calls {@code handle} any more. */
    private static native void released(long handle);
}
