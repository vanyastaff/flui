// Ask each enabled layout's OWN tables what Option-E and then `e` mean.
// `deadKeyState != 0` after the first press is the layout saying "dead key";
// the resolved character after the second press is what it would commit.
// Nothing here synthesizes an event — this reads the key-translation tables.
//
// Build + run:  clang -fobjc-arc -framework Cocoa -framework Carbon \
//                   -o layout_tables layout_tables.m
//               ./layout_tables
//
// Measured 2026-09-17:
//   LAYOUT com.apple.keylayout.US      Option-E -> ""   deadKeyState=1
//   LAYOUT com.apple.keylayout.US      then  e -> "é"   deadKeyState=65536
//   LAYOUT com.apple.keylayout.US      plain e -> "e"   deadKeyState=0
//   LAYOUT com.apple.keylayout.Russian Option-E -> "ќ"  deadKeyState=0
//   LAYOUT com.apple.keylayout.Russian then  e -> "у"   deadKeyState=0
//
// The U.S. layout does define the dead key, so "the layout has no dead key
// here" is not the reason the live drive did not compose. What the second
// press's deadKeyState=65536 means is *not* established here — it is reported
// as observed, and the argument above rests only on the first press (an empty
// string with a non-zero state) and the second press's resolved "é".
#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

// UCKeyTranslate's modifierKeyState is the high byte of a CGEventFlags value:
// Option = (kCGEventFlagMaskAlternate >> 16) == 0x08.
static NSString *translate(NSString *ident, CGEventFlags flags, UInt16 code, UInt32 *dead) {
  NSArray *list = CFBridgingRelease(TISCreateInputSourceList(
      (__bridge CFDictionaryRef)@{(__bridge NSString *)kTISPropertyInputSourceID: ident}, false));
  if (!list.count) return @"(no such source)";
  CFDataRef data = TISGetInputSourceProperty((__bridge TISInputSourceRef)list[0],
                                             kTISPropertyUnicodeKeyLayoutData);
  if (!data) return @"(source has no uchr layout data)";
  UniChar buf[8]; UniCharCount len = 0;
  OSStatus st = UCKeyTranslate((const UCKeyboardLayout *)CFDataGetBytePtr(data), code,
                               kUCKeyActionDown, (UInt32)((flags >> 16) & 0xFF),
                               LMGetKbdType(), 0, dead, 8, &len, buf);
  if (st != noErr) return [NSString stringWithFormat:@"(translate err %d)", (int)st];
  return [NSString stringWithCharacters:buf length:len];
}

int main(void) {
  @autoreleasepool {
    for (NSString *ident in @[@"com.apple.keylayout.US", @"com.apple.keylayout.Russian"]) {
      UInt32 dead = 0;
      NSString *first = translate(ident, kCGEventFlagMaskAlternate, 14, &dead);
      fprintf(stderr, "LAYOUT %-30s Option-E -> \"%s\"  deadKeyState=%u\n",
              [ident UTF8String], [first UTF8String], (unsigned)dead);
      UInt32 carry = dead;
      NSString *second = translate(ident, 0, 14, &carry);
      fprintf(stderr, "LAYOUT %-30s then  e -> \"%s\"  deadKeyState=%u\n",
              [ident UTF8String], [second UTF8String], (unsigned)carry);
      UInt32 plain = 0;
      NSString *pe = translate(ident, 0, 14, &plain);
      fprintf(stderr, "LAYOUT %-30s plain e -> \"%s\"  deadKeyState=%u\n",
              [ident UTF8String], [pe UTF8String], (unsigned)plain);
    }
    exit(0);
  }
}
