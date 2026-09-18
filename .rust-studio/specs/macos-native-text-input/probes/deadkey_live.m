// Does AppKit's real input method compose a dead key into a single commit?
// Minimal NSTextInputClient that logs every callback, driven with synthesized
// Option-E then 'e' through -interpretKeyEvents:.
//
// Build + run (staged as a .app because an unbundled NSWindow throws
// _CFBundleGetValueForInfoKey — the same floor the Rust probe documents):
//
//   clang -fobjc-arc -framework Cocoa -o DeadKey deadkey_live.m
//   mkdir -p DeadKey.app/Contents/MacOS
//   cp Info.plist DeadKey.app/Contents/Info.plist
//   cp DeadKey DeadKey.app/Contents/MacOS/DeadKey
//   ./DeadKey.app/Contents/MacOS/DeadKey
//
// Measured 2026-09-17 on this host, Russian layout active: two independent
// `insertText:` calls ("´" then "e"), no `setMarkedText:`, hasMarkedText=0 —
// no composition. See the plan's §10 for what that means and does not.
#import <Cocoa/Cocoa.h>

@interface ProbeView : NSView <NSTextInputClient> {
  NSMutableString *_marked;
}
@end

@implementation ProbeView
- (instancetype)initWithFrame:(NSRect)r {
  if ((self = [super initWithFrame:r])) { _marked = [NSMutableString new]; }
  return self;
}
- (BOOL)acceptsFirstResponder { return YES; }

// --- NSTextInputClient ---
- (void)insertText:(id)string replacementRange:(NSRange)rr {
  NSString *s = [string isKindOfClass:[NSAttributedString class]] ? [string string] : string;
  fprintf(stderr, "CALL insertText text=%s len=%lu replacementRange=(%lu,%lu)\n",
          [s UTF8String], (unsigned long)[s length],
          (unsigned long)rr.location, (unsigned long)rr.length);
  [_marked setString:@""];
}
- (void)setMarkedText:(id)string selectedRange:(NSRange)sel replacementRange:(NSRange)rr {
  NSString *s = [string isKindOfClass:[NSAttributedString class]] ? [string string] : string;
  fprintf(stderr, "CALL setMarkedText text=%s len=%lu selected=(%lu,%lu)\n",
          [s UTF8String], (unsigned long)[s length],
          (unsigned long)sel.location, (unsigned long)sel.length);
  [_marked setString:s];
}
- (void)unmarkText {
  fprintf(stderr, "CALL unmarkText (marked was \"%s\")\n", [_marked UTF8String]);
  [_marked setString:@""];
}
- (NSRange)selectedRange { return NSMakeRange(0, 0); }
- (NSRange)markedRange {
  if ([_marked length] == 0) return NSMakeRange(NSNotFound, 0);
  return NSMakeRange(0, [_marked length]);
}
- (BOOL)hasMarkedText { return [_marked length] > 0; }
- (NSAttributedString *)attributedSubstringForProposedRange:(NSRange)r actualRange:(NSRangePointer)a {
  return nil;
}
- (NSArray<NSAttributedStringKey> *)validAttributesForMarkedText { return @[]; }
- (NSRect)firstRectForCharacterRange:(NSRange)r actualRange:(NSRangePointer)a {
  return NSMakeRect(10, 10, 5, 18);
}
- (NSUInteger)characterIndexForPoint:(NSPoint)p { return 0; }
- (void)doCommandBySelector:(SEL)sel {
  fprintf(stderr, "CALL doCommandBySelector %s\n", sel_getName(sel));
}
@end

static NSEvent *key(NSString *chars, NSString *ignoring, NSEventModifierFlags flags, unsigned short code) {
  return [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSMakePoint(10,10)
                     modifierFlags:flags timestamp:0 windowNumber:0 context:nil
                        characters:chars charactersIgnoringModifiers:ignoring
                        isARepeat:NO keyCode:code];
}

static void run(void) {
  [NSApplication sharedApplication];
  [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
  NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,400,200)
      styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
  ProbeView *v = [[ProbeView alloc] initWithFrame:NSMakeRect(0,0,400,200)];
  [w setContentView:v];
  [w makeKeyAndOrderFront:nil];
  [NSApp activateIgnoringOtherApps:YES];
  [w makeFirstResponder:v];

  fprintf(stderr, "PROBE current input source id=%s\n",
    [[[[NSTextInputContext alloc] initWithClient:v] selectedKeyboardInputSource] UTF8String]);

  fprintf(stderr, "PROBE -- press 1: Option-E (dead acute), keyCode 14\n");
  [v interpretKeyEvents:@[key(@"´", @"e", NSEventModifierFlagOption, 14)]];
  fprintf(stderr, "PROBE -- press 2: e, keyCode 14\n");
  [v interpretKeyEvents:@[key(@"e", @"e", 0, 14)]];
  fprintf(stderr, "PROBE -- done. hasMarkedText=%d\n", (int)[v hasMarkedText]);
  exit(0);
}

int main(void) {
  @autoreleasepool { run(); }
  return 0;
}
