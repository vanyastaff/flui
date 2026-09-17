// Is `setNeedsDisplay:YES` issued from INSIDE drawRect: honoured?
//
// probe2 established: a timer (outside the run loop's display pass) poking
// setNeedsDisplay:YES drives drawRect: forever; the view's own layer/CAMetalLayer
// setup is innocent. The FLUI native backend re-arms the same way every other
// GPU app does — from inside the frame, i.e. from inside its drawRect: — and
// gets exactly one draw and then nothing.
//
// Arms, all in this one binary so they run on the same machine in one go:
//   7  self-re-arm inside drawRect: (FLUI's shape), no external poke at all
//   8  no re-arm, no poke (control: how many draws does the initial pass give?)
//   9  timer poke only, no self-re-arm (control: the arm that is known to work)
//  10  both self-re-arm AND timer poke
//
// Reports at exit; prints the needsDisplay flag immediately after the re-arm,
// still inside the display pass, and again at the top of the next drawRect:.

#import <Cocoa/Cocoa.h>

static int g_requests = 0, g_draws = 0, g_arm = 0;
static int g_needs_display_after_rearm = -1;
static BOOL g_self_rearm = NO, g_timer_poke = NO, g_deferred_rearm = NO, g_layer_rearm = NO;
static __weak NSWindow *g_win = nil;
static __weak NSView *g_view = nil;

@interface ProbeView : NSView
@end

@implementation ProbeView
- (void)drawRect:(NSRect)r {
  g_draws++;
  NSLog(@"    drawRect #%d (arm=%d) needsDisplay-at-entry=%d", g_draws, g_arm,
        (int)[self needsDisplay]);
  if (g_self_rearm) {
    [self setNeedsDisplay:YES];
    g_needs_display_after_rearm = (int)[self needsDisplay];
    NSLog(@"    re-armed from inside drawRect: needsDisplay=%d", g_needs_display_after_rearm);
  }
  if (g_layer_rearm) {
    // Re-arm via the backing layer instead of the view: the layer commit
    // happens after the view's own flag is cleared, so the dirty bit may
    // survive. No dispatch, no lifetime concern — if it works, this is a
    // one-line fix.
    self.wantsLayer = YES;
    [self.layer setNeedsDisplay];
    NSLog(@"    re-armed via layer: layer.needsDisplay=%d", (int)[self.layer needsDisplay]);
  }
  if (g_deferred_rearm) {
    // Same intent, different timing: hand the re-arm to the main queue so it
    // runs AFTER this display pass unwinds, instead of during it.
    __weak ProbeView *weak_self = self;
    dispatch_async(dispatch_get_main_queue(), ^{
      [weak_self setNeedsDisplay:YES];
    });
  }
}
@end

int main(int argc, char **argv) {
  @autoreleasepool {
    if (argc > 1) g_arm = atoi(argv[1]);
    g_self_rearm = (g_arm == 7 || g_arm == 10);
    g_deferred_rearm = (g_arm == 11);
    g_layer_rearm = (g_arm == 12);
    g_timer_poke = (g_arm == 9 || g_arm == 10);

    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];

    NSRect frame = NSMakeRect(300, 300, 500, 400);
    NSWindow *win = [[NSWindow alloc]
        initWithContentRect:frame
                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
                    backing:NSBackingStoreBuffered
                      defer:NO];
    ProbeView *v = [[ProbeView alloc] initWithFrame:frame];
    [win setContentView:v];
    [win makeKeyAndOrderFront:nil];
    [app activateIgnoringOtherApps:YES];
    g_win = win;
    g_view = v;

    // The poke, when armed, lives on its own timer — i.e. it runs OUTSIDE any
    // display pass, which is the one condition probe2 proved sufficient.
    if (g_timer_poke) {
      [NSTimer scheduledTimerWithTimeInterval:0.25 repeats:YES block:^(NSTimer *t) {
        NSView *view = g_view;
        if (view == nil) return;
        g_requests++;
        [view setNeedsDisplay:YES];
      }];
    }

    [NSTimer scheduledTimerWithTimeInterval:4.0 repeats:NO block:^(NSTimer *t) {
      NSWindow *w = g_win;
      NSLog(@"RESULT arm=%d self_rearm=%d timer_poke=%d requests=%d draws=%d "
             "occlusion=%lu needsDisplay_after_rearm=%d",
            g_arm, (int)g_self_rearm, (int)(g_timer_poke || g_deferred_rearm), g_requests, g_draws,
            (unsigned long)[w occlusionState], g_needs_display_after_rearm);
      exit(0);
    }];
    [app run];
  }
  return 0;
}
