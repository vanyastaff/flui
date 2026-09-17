// The display's actual refresh rate, for interpreting a measured cadence.
#import <Cocoa/Cocoa.h>
int main(void) {
  @autoreleasepool {
    for (NSScreen *s in [NSScreen screens]) {
      NSDictionary *d = [s deviceDescription];
      NSNumber *n = d[@"NSDeviceRefreshRate"];
      CGDirectDisplayID did = [[d[@"NSScreenNumber"] description] intValue];
      CGDisplayModeRef m = CGDisplayCopyDisplayMode(did);
      double hz = m ? CGDisplayModeGetRefreshRate(m) : 0.0;
      if (m) CGDisplayModeRelease(m);
      printf("screen id=%u deviceRefreshRate=%.3f Hz  CGRefreshRate=%.3f Hz  maxFPS=%ld  frames=%ldx%ld\n",
             did, n ? [n doubleValue] : -1.0, hz, (long)[s maximumFramesPerSecond],
             (long)CGDisplayPixelsWide(did), (long)CGDisplayPixelsHigh(did));
    }
  }
  return 0;
}
