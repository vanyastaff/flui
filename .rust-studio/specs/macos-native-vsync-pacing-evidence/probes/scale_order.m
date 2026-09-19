// Does an NSWindow have a screen (and the right backing scale) before it is
// ordered front, and which refresh-rate API should the backend read?
//
// Two questions, one run:
//
//   1. FLUI creates its window and reads `backingScaleFactor` BEFORE
//      `makeKeyAndOrderFront:`. If `-[NSWindow screen]` is nil at that moment,
//      the scale came from a window that was not on any display yet, and the
//      read belongs after the window is on screen. Decisive on ANY display,
//      including a 1x one where the scale value itself cannot distinguish —
//      so this probe prints `screen == nil`, not just the scale.
//
//   2. `refresh_period()` is unimplemented for macOS, so the runner paces
//      against its 16.667 ms default on a 100 Hz panel. This prints every
//      candidate source with its value, so the implementation reads the one
//      that is actually correct here rather than the first one that compiles.
//
// Build: clang -fobjc-arc -framework Cocoa -o /tmp/scale_order probes/scale_order.m
// Run:   /tmp/scale_order

#import <Cocoa/Cocoa.h>

static void report(const char *label, NSWindow *win, NSScreen *main_screen) {
  NSScreen *s = [win screen];
  printf("%-28s screen=%-6s scale=%.2f  screen.scale=%s  screen.maxFPS=%s\n", label,
         s == nil ? "nil" : "live", (double)[win backingScaleFactor],
         s == nil ? "-" : [[NSString stringWithFormat:@"%.2f", [s backingScaleFactor]] UTF8String],
         s == nil ? "-"
                  : [[NSString stringWithFormat:@"%ld", (long)[s maximumFramesPerSecond]] UTF8String]);
  (void)main_screen;
}

int main(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
    [NSApp activateIgnoringOtherApps:YES];

    // The window FLUI builds: contentRect, the same style mask family, buffered.
    NSRect rect = NSMakeRect(100, 100, 480, 320);
    NSWindowStyleMask mask = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                             NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable;
    NSWindow *win = [[NSWindow alloc] initWithContentRect:rect
                                                styleMask:mask
                                                  backing:NSBackingStoreBuffered
                                                    defer:NO];

    NSScreen *main_screen = [NSScreen mainScreen];

    printf("== 1. screen presence and backing scale, across the creation order ==\n");
    printf("NSScreen.mainScreen          screen=live   scale=%.2f  maxFPS=%ld  frame=%.0fx%.0f\n",
           (double)[main_screen backingScaleFactor], (long)[main_screen maximumFramesPerSecond],
           main_screen.frame.size.width, main_screen.frame.size.height);
    printf("screens count                %lu\n", (unsigned long)[[NSScreen screens] count]);

    // (a) what FLUI reads today: right after init, before ordering front.
    report("a) after init", win, main_screen);

    // (b) after the window is actually on screen — what FLUI does next.
    [win makeKeyAndOrderFront:nil];
    report("b) after makeKeyAndOrderF", win, main_screen);

    // (c) after center, which FLUI also does.
    [win center];
    report("c) after center", win, main_screen);

    printf("\n== 2. refresh-rate candidates, on the screen the window is on ==\n");
    NSScreen *s = [win screen];
    if (s != nil) {
      printf("NSScreen.maximumFramesPerSecond   %ld\n", (long)[s maximumFramesPerSecond]);

      NSNumber *screen_number = [[s deviceDescription] objectForKey:@"NSScreenNumber"];
      printf("NSScreenNumber                    %u\n", [screen_number unsignedIntValue]);

      CGDirectDisplayID did = (CGDirectDisplayID)[screen_number unsignedIntValue];
      CGDisplayModeRef mode = CGDisplayCopyDisplayMode(did);
      if (mode != NULL) {
        double hz = CGDisplayModeGetRefreshRate(mode);
        printf("CGDisplayModeGetRefreshRate       %.6f\n", hz);
        printf("CGDisplayModeGetPixelWidth/Height %zu x %zu\n", CGDisplayModeGetPixelWidth(mode),
               CGDisplayModeGetPixelHeight(mode));
        printf("CGDisplayModeGetWidth/Height      %zu x %zu\n", CGDisplayModeGetWidth(mode),
               CGDisplayModeGetHeight(mode));
        printf("CGDisplayIsMain                   %d\n", CGDisplayIsMain(did));
        CFRelease(mode);
      } else {
        printf("CGDisplayCopyDisplayMode          NULL\n");
      }
      printf("CGDisplayModeGetRefreshRate(main) %.6f\n",
             CGDisplayModeGetRefreshRate(CGDisplayCopyDisplayMode(CGMainDisplayID())));

      // Derived periods, so the arithmetic the backend will do is checked here.
      double hz = CGDisplayModeGetRefreshRate(CGDisplayCopyDisplayMode(CGMainDisplayID()));
      if (hz > 0) {
        printf("period (CG, ms)                   %.4f\n", 1000.0 / hz);
      }
      printf("period (maxFPS, ms)               %.4f\n",
             [s maximumFramesPerSecond] > 0 ? 1000.0 / (double)[s maximumFramesPerSecond] : 0.0);
    } else {
      printf("window has no screen after ordering front\n");
    }

    printf("\nSCALE_ORDER_PROBE_DONE\n");
    [win orderOut:nil];
  }
  return 0;
}
