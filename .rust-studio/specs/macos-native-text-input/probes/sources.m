// Which keyboard input sources exist, are enabled, and are selected.
//
// Build + run:  clang -fobjc-arc -framework Cocoa -framework Carbon -o sources sources.m
//               ./sources
//
// Measured 2026-09-17 on this host: exactly two, both raw keylayouts —
// com.apple.keylayout.US and com.apple.keylayout.Russian, both enabled=1
// selected=1. No input source of type kTISTypeInputMethod is present at all,
// which rules out retrying the dead-key drive (or a CJK conversion) with a
// different input source: there is nothing else to select. Enabling one is a
// System Settings change, not something a probe may make for its user.
#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>
int main(void) {
  @autoreleasepool {
    NSArray *all = CFBridgingRelease(TISCreateInputSourceList(NULL, false));
    for (id s in all) {
      TISInputSourceRef r = (__bridge TISInputSourceRef)s;
      NSString *idv = (__bridge NSString *)TISGetInputSourceProperty(r, kTISPropertyInputSourceID);
      NSString *cat = (__bridge NSString *)TISGetInputSourceProperty(r, kTISPropertyInputSourceCategory);
      Boolean sel = TISGetInputSourceProperty(r, kTISPropertyInputSourceIsSelected) != NULL;
      const void *en = TISGetInputSourceProperty(r, kTISPropertyInputSourceIsEnabled);
      if (![cat isEqualToString:(__bridge NSString *)kTISCategoryKeyboardInputSource]) continue;
      fprintf(stderr, "SRC id=%s enabled=%d selected=%d\n", [idv UTF8String],
              en ? CFBooleanGetValue((CFBooleanRef)en) : -1, (int)sel);
    }
    exit(0);
  }
}
