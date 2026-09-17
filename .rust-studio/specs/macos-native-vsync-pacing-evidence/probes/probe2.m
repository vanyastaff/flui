#import <Cocoa/Cocoa.h>
#import <QuartzCore/QuartzCore.h>
#import <Metal/Metal.h>

static int g_requests = 0, g_draws = 0;
static int g_arm = 0;
static BOOL g_attached = NO;
static __weak NSWindow *g_win = nil;
static __weak NSView   *g_view = nil;

static void attach(void) {
  if (g_attached) return;
  NSView *v = g_view;
  if (v == nil) return;
  id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
  CAMetalLayer *ml = [CAMetalLayer layer];
  ml.device = dev;
  ml.pixelFormat = MTLPixelFormatBGRA8Unorm;
  ml.framebufferOnly = YES;
  [v setWantsLayer:YES];
  CALayer *root = [v layer];
  ml.frame = root.bounds;
  ml.contentsScale = root.contentsScale;
  [root addSublayer:ml];                       // <- raw-window-metal's real path
  g_attached = YES;
  NSLog(@"    ** attached CAMetalLayer as SUBLAYER of root layer **");
}

@interface ProbeView : NSView
@end
@implementation ProbeView
- (void)drawRect:(NSRect)r {
  g_draws++;
  NSLog(@"    drawRect #%d (arm=%d)", g_draws, g_arm);
  // arm 4: the real app converts the view to layer-backing from INSIDE its own
  // display pass (the wgpu surface is created while frame 1 renders).
  if (g_arm == 4) attach();
}
@end

int main(int argc, char **argv) {
  @autoreleasepool {
    if (argc > 1) g_arm = atoi(argv[1]);
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];

    NSRect frame = NSMakeRect(300, 300, 500, 400);
    NSWindow *win = [[NSWindow alloc]
        initWithContentRect:frame
                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
                    backing:NSBackingStoreBuffered defer:NO];
    ProbeView *v = [[ProbeView alloc] initWithFrame:frame];
    if (g_arm == 6) {
      // FLUI's real ordering: order the window front with AppKit's DEFAULT
      // content view, center it, and only THEN install our view.
      [win makeKeyAndOrderFront:nil];
      [win center];
      [win setContentView:v];
    } else {
      [win setContentView:v];
      [win makeKeyAndOrderFront:nil];
    }
    [app activateIgnoringOtherApps:YES];
    g_win = win; g_view = v;

    // arm 3: same attach, but from a timer — outside any display pass.
    if (g_arm == 3) {
      [NSTimer scheduledTimerWithTimeInterval:0.75 repeats:NO block:^(NSTimer *t) { attach(); }];
    }

    [NSTimer scheduledTimerWithTimeInterval:0.25 repeats:YES block:^(NSTimer *t) {
      NSWindow *w = g_win; NSView *view = g_view;
      if (w == nil || view == nil) return;
      g_requests++;
      NSLog(@"  req #%d needsDisplay=%d occlusion=%lu visibleBit=%d", g_requests,
            (int)[view needsDisplay], (unsigned long)[w occlusionState],
            (int)(([w occlusionState] & NSWindowOcclusionStateVisible) != 0));
      [view setNeedsDisplay:YES];
    }];

    [NSTimer scheduledTimerWithTimeInterval:4.0 repeats:NO block:^(NSTimer *t) {
      NSLog(@"RESULT arm=%d requests=%d draws=%d attached=%d", g_arm, g_requests, g_draws, (int)g_attached);
      exit(0);
    }];
    [app run];
  }
  return 0;
}
