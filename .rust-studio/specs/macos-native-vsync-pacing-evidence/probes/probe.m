#import <Cocoa/Cocoa.h>
#import <QuartzCore/QuartzCore.h>
#import <Metal/Metal.h>

static int g_requests = 0, g_draws = 0;
static int g_arm = 0;              // 0 = plain, 1 = wantsLayer only, 2 = layer-hosting CAMetalLayer
static __weak NSWindow *g_win = nil;
static __weak NSView   *g_view = nil;

@interface ProbeView : NSView
@end
@implementation ProbeView
- (void)drawRect:(NSRect)r {
  g_draws++;
  NSLog(@"    drawRect #%d", g_draws);
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
                    backing:NSBackingStoreBuffered
                      defer:NO];
    ProbeView *v = [[ProbeView alloc] initWithFrame:frame];
    [win setContentView:v];
    [win makeKeyAndOrderFront:nil];
    [app activateIgnoringOtherApps:YES];
    g_win = win; g_view = v;

    NSLog(@"ARM %d start: wantsLayer=%d layer=%p occlusion=%lu",
          g_arm, (int)[v wantsLayer], (__bridge void *)[v layer], (unsigned long)[win occlusionState]);

    // A: attach the layer AFTER the first display pass (mimics wgpu creating the
    // surface during the first submitted frame, i.e. after drawRect: already ran).
    if (g_arm == 1 || g_arm == 2) {
      [NSTimer scheduledTimerWithTimeInterval:0.75 repeats:NO block:^(NSTimer *t) {
        NSView *view = g_view;
        if (view == nil) return;
        if (g_arm == 2) {
          id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
          CAMetalLayer *layer = [CAMetalLayer layer];
          layer.device = dev;
          layer.pixelFormat = MTLPixelFormatBGRA8Unorm;
          layer.framebufferOnly = YES;
          layer.drawableSize = NSMakeSize(frame.size.width, frame.size.height);
          [view setWantsLayer:YES];
          [view setLayer:layer];            // <- layer-HOSTING: AppKit stops drawing it
          NSLog(@"    ** arm 2: attached CAMetalLayer via setLayer: **");
        } else {
          [view setWantsLayer:YES];         // <- layer-BACKED: AppKit still calls drawRect:
          NSLog(@"    ** arm 1: setWantsLayer:YES (AppKit-managed backing layer) **");
        }
        NSLog(@"    now wantsLayer=%d layer=%p layerHosting=%d",
              (int)[view wantsLayer], (__bridge void *)[view layer],
              (int)([view layer] != nil && [[view layer] isKindOfClass:[CAMetalLayer class]]));
      }];
    }

    // Drive redraws the way FLUI's request_redraw() does: setNeedsDisplay:, nothing else.
    [NSTimer scheduledTimerWithTimeInterval:0.25 repeats:YES block:^(NSTimer *t) {
      NSWindow *w = g_win; NSView *view = g_view;
      if (w == nil || view == nil) return;
      g_requests++;
      NSUInteger occ = [w occlusionState];
      NSLog(@"  req #%d occlusion=%lu visibleBit=%d", g_requests, (unsigned long)occ,
            (int)((occ & NSWindowOcclusionStateVisible) != 0));
      [view setNeedsDisplay:YES];
    }];

    [NSTimer scheduledTimerWithTimeInterval:4.0 repeats:NO block:^(NSTimer *t) {
      NSLog(@"RESULT arm=%d requests=%d draws=%d", g_arm, g_requests, g_draws);
      exit(0);
    }];

    [app run];
  }
  return 0;
}
