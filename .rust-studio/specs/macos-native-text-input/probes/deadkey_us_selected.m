// Same drive as deadkey_live.m, but select the U.S. layout first and restore
// the user's original input source immediately afterwards — the hypothesis
// being that the Russian layout, not the input context, was why nothing
// composed.
//
// Build + run: identical to deadkey_live.m (same Info.plist staging).
//
// Measured 2026-09-17, with TISSelectInputSource(com.apple.keylayout.US)
// returning status 0 and the original source restored afterwards: still two
// independent `insertText:` calls ("´" then "e"), no `setMarkedText:`,
// hasMarkedText=0. Selecting the U.S. layout does not open a composition
// either — so the layout is not what is missing, which layout_tables.m then
// confirms from the other side (the U.S. tables DO define the dead key).
#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

@interface ProbeView : NSView <NSTextInputClient> { NSMutableString *_marked; }
@end
@implementation ProbeView
- (instancetype)initWithFrame:(NSRect)r {
  if ((self = [super initWithFrame:r])) { _marked = [NSMutableString new]; }
  return self;
}
- (BOOL)acceptsFirstResponder { return YES; }
- (void)insertText:(id)str replacementRange:(NSRange)rr {
  NSString *s = [str isKindOfClass:[NSAttributedString class]] ? [str string] : str;
  fprintf(stderr, "CALL insertText text=%s\n", [s UTF8String]);
  [_marked setString:@""];
}
- (void)setMarkedText:(id)s selectedRange:(NSRange)sel replacementRange:(NSRange)rr {
  NSString *t = [s isKindOfClass:[NSAttributedString class]] ? [s string] : s;
  fprintf(stderr, "CALL setMarkedText text=%s selected=(%lu,%lu)\n", [t UTF8String],
          (unsigned long)sel.location, (unsigned long)sel.length);
  [_marked setString:t];
}
- (void)unmarkText { fprintf(stderr, "CALL unmarkText was=\"%s\"\n", [_marked UTF8String]); [_marked setString:@""]; }
- (NSRange)selectedRange { return NSMakeRange(0,0); }
- (NSRange)markedRange { return [_marked length] ? NSMakeRange(0,[_marked length]) : NSMakeRange(NSNotFound,0); }
- (BOOL)hasMarkedText { return [_marked length] > 0; }
- (NSAttributedString *)attributedSubstringForProposedRange:(NSRange)r actualRange:(NSRangePointer)a { return nil; }
- (NSArray<NSAttributedStringKey> *)validAttributesForMarkedText { return @[]; }
- (NSRect)firstRectForCharacterRange:(NSRange)r actualRange:(NSRangePointer)a { return NSMakeRect(10,10,5,18); }
- (NSUInteger)characterIndexForPoint:(NSPoint)p { return 0; }
- (void)doCommandBySelector:(SEL)sel { fprintf(stderr, "CALL doCommandBySelector %s\n", sel_getName(sel)); }
@end

static NSString *current_source_id(void) {
  TISInputSourceRef s = TISCopyCurrentKeyboardInputSource();
  if (!s) return @"(none)";
  NSString *v = (__bridge NSString *)TISGetInputSourceProperty(s, kTISPropertyInputSourceID);
  return v ?: @"(none)";
}

static TISInputSourceRef source_with_id(NSString *ident) {
  NSDictionary *f = @{(__bridge NSString *)kTISPropertyInputSourceID: ident};
  NSArray *list = CFBridgingRelease(TISCreateInputSourceList((__bridge CFDictionaryRef)f, false));
  return list.count ? (__bridge TISInputSourceRef)list[0] : NULL;
}

static NSEvent *key(NSString *chars, NSString *ign, NSEventModifierFlags f, unsigned short code) {
  return [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSMakePoint(10,10)
                     modifierFlags:f timestamp:0 windowNumber:0 context:nil
                        characters:chars charactersIgnoringModifiers:ign
                        isARepeat:NO keyCode:code];
}

int main(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,400,200)
        styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    ProbeView *v = [[ProbeView alloc] initWithFrame:NSMakeRect(0,0,400,200)];
    [w setContentView:v];
    [w makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
    [w makeFirstResponder:v];

    NSString *original = current_source_id();
    fprintf(stderr, "PROBE original input source = %s\n", [original UTF8String]);

    TISInputSourceRef us = source_with_id(@"com.apple.keylayout.US");
    if (!us) { fprintf(stderr, "PROBE US layout not found; aborting\n"); return 2; }
    OSStatus st = TISSelectInputSource(us);
    fprintf(stderr, "PROBE TISSelectInputSource(US) status=%d now=%s\n",
            (int)st, [current_source_id() UTF8String]);

    fprintf(stderr, "PROBE -- press 1: Option-E (dead acute), keyCode 14\n");
    [v interpretKeyEvents:@[key(@"´", @"e", NSEventModifierFlagOption, 14)]];
    fprintf(stderr, "PROBE -- press 2: e, keyCode 14\n");
    [v interpretKeyEvents:@[key(@"e", @"e", 0, 14)]];
    fprintf(stderr, "PROBE -- done. hasMarkedText=%d\n", (int)[v hasMarkedText]);

    TISInputSourceRef back = source_with_id(original);
    if (back) { TISSelectInputSource(back); }
    fprintf(stderr, "PROBE restored input source = %s\n", [current_source_id() UTF8String]);
    exit(0);
  }
}
