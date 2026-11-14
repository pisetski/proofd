#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

static void logMsg(NSString *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    NSString *msg = [[NSString alloc] initWithFormat:fmt arguments:args];
    va_end(args);
    NSString *logDir = [NSString stringWithFormat:@"%s/Library/Logs", getenv("HOME")];
    NSString *logPath = [logDir stringByAppendingPathComponent:@"proofd-service.log"];
    NSString *timestamp = [[NSDate date] description];
    NSString *line = [NSString stringWithFormat:@"[%@] %@\n", timestamp, msg];
    NSData *data = [line dataUsingEncoding:NSUTF8StringEncoding];
    NSFileHandle *fh = [NSFileHandle fileHandleForWritingAtPath:logPath];
    if (!fh) {
        [[NSFileManager defaultManager] createDirectoryAtPath:logDir withIntermediateDirectories:YES attributes:nil error:nil];
        [data writeToFile:logPath atomically:YES];
    } else {
        [fh seekToEndOfFile];
        [fh writeData:data];
        [fh closeFile];
    }
}

static const CGFloat kIndicatorSize = 30;
static const CGFloat kIndicatorCursorOffset = 14;
static const CGFloat kPillPadding = 10;
static const CGFloat kPillSymbolGap = 5;
static const NSTimeInterval kFadeIn = 0.15;
static const NSTimeInterval kCrossfade = 0.12;
static const NSTimeInterval kFadeOut = 0.25;
static const NSTimeInterval kSuccessHold = 0.8;
static const NSTimeInterval kFailureHold = 2.5;

static const NSTimeInterval kDefaultServicesTimeout = 30;
static const NSTimeInterval kServicesTimeoutMargin = 1;
static const NSTimeInterval kTerminateGrace = 0.5;

// Spins the run loop, not a blocking wait, so the indicator keeps animating and following the cursor.
static BOOL waitForExit(NSTask *task, NSDate *deadline) {
    while (task.isRunning && deadline.timeIntervalSinceNow > 0) {
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
    return !task.isRunning;
}

static NSTimeInterval servicesTimeout(void) {
    NSArray *services = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSServices"];
    id milliseconds = [services.firstObject objectForKey:@"NSTimeout"];
    return milliseconds ? [milliseconds doubleValue] / 1000 : kDefaultServicesTimeout;
}

static NSPoint indicatorOriginNearCursor(NSSize size) {
    NSPoint cursor = [NSEvent mouseLocation];
    NSPoint origin = NSMakePoint(cursor.x + kIndicatorCursorOffset,
                                 cursor.y - kIndicatorCursorOffset - size.height);
    for (NSScreen *screen in [NSScreen screens]) {
        if (!NSMouseInRect(cursor, screen.frame, NO)) continue;
        NSRect bounds = screen.visibleFrame;
        if (origin.x + size.width > NSMaxX(bounds)) origin.x = cursor.x - kIndicatorCursorOffset - size.width;
        if (origin.y < NSMinY(bounds)) origin.y = cursor.y + kIndicatorCursorOffset;
        origin.x = MIN(MAX(origin.x, NSMinX(bounds)), NSMaxX(bounds) - size.width);
        origin.y = MIN(MAX(origin.y, NSMinY(bounds)), NSMaxY(bounds) - size.height);
        break;
    }
    return origin;
}

static void centerInLeadingSquare(NSView *view) {
    [view setFrameOrigin:NSMakePoint((kIndicatorSize - NSWidth(view.frame)) / 2,
                                     (kIndicatorSize - NSHeight(view.frame)) / 2)];
}

// Matches only proofd's fixed error prefixes: the rest of the line can quote the provider's
// own output (e.g. an HTTP "404 Not Found" body), which must not pick the reason.
// tests/proofread_cli.rs parses this table and checks it against real proofd errors.
static NSString *shortFailureReason(NSString *proofdStderr) {
    NSString *message = @"";
    for (NSString *line in [proofdStderr componentsSeparatedByCharactersInSet:[NSCharacterSet newlineCharacterSet]]) {
        if ([line hasPrefix:@"proofd: "]) message = [line substringFromIndex:@"proofd: ".length];
    }
    NSArray<NSArray<NSString *> *> *reasons = @[
        @[@"provider error: provider timed out", @"Timed out"],
        @[@"provider error: transport error: provider_bin", @"Can't start provider"],
        @[@"provider error: transport error: spawn", @"Can't start provider"],
        @[@"provider error: transport error: openai-compat", @"Can't reach provider"],
        @[@"provider error: provider failed", @"Provider error"],
        @[@"invalid config at", @"Check proofd config"],
        @[@"failed to read config at", @"Check proofd config"],
        @[@"input too large", @"Selection too long"],
        @[@"no text to proofread", @"No text selected"],
        @[@"provider returned empty output", @"Empty response"],
        @[@"provider error: provider returned empty output", @"Empty response"],
    ];
    for (NSArray<NSString *> *reason in reasons) {
        if ([message hasPrefix:reason[0]]) return reason[1];
    }
    return @"Proofread failed";
}

static NSPanel *makeIndicatorPanel(void) {
    NSSize size = NSMakeSize(kIndicatorSize, kIndicatorSize);
    NSPanel *panel = [[NSPanel alloc] initWithContentRect:(NSRect){indicatorOriginNearCursor(size), size}
                                                styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                  backing:NSBackingStoreBuffered
                                                    defer:NO];
    panel.releasedWhenClosed = NO;
    panel.level = NSPopUpMenuWindowLevel;
    panel.opaque = NO;
    panel.backgroundColor = [NSColor clearColor];
    panel.hasShadow = YES;
    panel.ignoresMouseEvents = YES;
    panel.hidesOnDeactivate = NO;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                               NSWindowCollectionBehaviorFullScreenAuxiliary |
                               NSWindowCollectionBehaviorStationary;
    return panel;
}

static NSColor *appearanceColor(NSColor *light, NSColor *dark) {
    return [NSColor colorWithName:nil dynamicProvider:^NSColor *(NSAppearance *appearance) {
        BOOL isDark = [[appearance bestMatchFromAppearancesWithNames:@[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]]
            isEqualToString:NSAppearanceNameDarkAqua];
        return isDark ? dark : light;
    }];
}

@interface BadgeView : NSView
@end

@implementation BadgeView

- (BOOL)isOpaque {
    return NO;
}

- (void)drawRect:(NSRect)dirtyRect {
    static const CGFloat rimWidth = 0.5;
    NSRect shape = NSInsetRect(self.bounds, rimWidth / 2, rimWidth / 2);
    CGFloat radius = NSHeight(shape) / 2;
    NSBezierPath *capsule = [NSBezierPath bezierPathWithRoundedRect:shape xRadius:radius yRadius:radius];
    [appearanceColor([NSColor whiteColor], [NSColor colorWithWhite:0.2 alpha:1]) setFill];
    [capsule fill];
    capsule.lineWidth = rimWidth;
    [appearanceColor([NSColor colorWithWhite:0 alpha:0.12], [NSColor colorWithWhite:1 alpha:0.16]) setStroke];
    [capsule stroke];
}

@end

static NSImageView *makeOutcomeSymbol(BOOL success) {
    NSString *name = success ? @"checkmark" : @"exclamationmark.triangle.fill";
    NSString *description = success ? @"Proofread" : @"Proofreading failed";
    NSImageView *symbol = [NSImageView imageViewWithImage:[NSImage imageWithSystemSymbolName:name
                                                                     accessibilityDescription:description]];
    symbol.symbolConfiguration = [NSImageSymbolConfiguration configurationWithPointSize:13 weight:NSFontWeightSemibold];
    symbol.contentTintColor = success ? [NSColor systemGreenColor] : [NSColor systemOrangeColor];
    [symbol sizeToFit];
    centerInLeadingSquare(symbol);
    return symbol;
}

static NSTextField *makeReasonLabel(NSString *reason) {
    NSTextField *label = [NSTextField labelWithString:reason];
    label.font = [NSFont systemFontOfSize:12 weight:NSFontWeightMedium];
    label.textColor = [NSColor labelColor];
    [label sizeToFit];
    return label;
}

static NSSize layOutPill(NSImageView *symbol, NSTextField *label) {
    CGFloat textInset = (NSWidth(label.frame) - ceil(label.attributedStringValue.size.width)) / 2;
    [symbol setFrameOrigin:NSMakePoint(kPillPadding, NSMinY(symbol.frame))];
    [label setFrameOrigin:NSMakePoint(NSMaxX(symbol.frame) + kPillSymbolGap - textInset,
                                      (kIndicatorSize - NSHeight(label.frame)) / 2)];
    return NSMakeSize(NSMaxX(label.frame) - textInset + kPillPadding, kIndicatorSize);
}

@interface CursorBusyIndicator : NSObject
- (void)finishWithSuccess;
- (void)finishWithFailure:(NSString *)reason;
@end

@implementation CursorBusyIndicator {
    NSPanel *_panel;
    BadgeView *_badge;
    NSProgressIndicator *_spinner;
    NSTimer *_followCursor;
    BOOL _closed;
}

static __weak CursorBusyIndicator *visibleIndicator;

- (instancetype)init {
    if (!(self = [super init])) return nil;
    [visibleIndicator close];
    visibleIndicator = self;
    _panel = makeIndicatorPanel();
    _badge = [[BadgeView alloc] initWithFrame:NSMakeRect(0, 0, kIndicatorSize, kIndicatorSize)];
    _badge.wantsLayer = YES;
    _spinner = [[NSProgressIndicator alloc] init];
    _spinner.style = NSProgressIndicatorStyleSpinning;
    _spinner.controlSize = NSControlSizeSmall;
    _spinner.usesThreadedAnimation = YES;
    [_spinner sizeToFit];
    centerInLeadingSquare(_spinner);
    [_badge addSubview:_spinner];
    _panel.contentView = _badge;
    [_spinner startAnimation:nil];
    [self redrawShadow];

    _panel.alphaValue = 0;
    [_panel orderFrontRegardless];
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = kFadeIn;
        self->_panel.animator.alphaValue = 1;
    }];

    NSPanel *panel = _panel;
    // A timer, not a mouse-moved monitor: the service method spins the run loop while proofd runs but never dispatches events.
    _followCursor = [NSTimer scheduledTimerWithTimeInterval:1.0 / 60
                                                    repeats:YES
                                                      block:^(NSTimer *timer) {
        NSPoint origin = indicatorOriginNearCursor(panel.frame.size);
        if (!NSEqualPoints(origin, panel.frame.origin)) [panel setFrameOrigin:origin];
    }];
    return self;
}

// The window server derives the shadow from drawn alpha, so it must follow every shape change.
- (void)redrawShadow {
    [_panel display];
    [_panel invalidateShadow];
}

- (void)finishWithSuccess {
    [self showOutcome:makeOutcomeSymbol(YES) reason:nil hold:kSuccessHold];
}

- (void)finishWithFailure:(NSString *)reason {
    [self showOutcome:makeOutcomeSymbol(NO) reason:makeReasonLabel(reason) hold:kFailureHold];
}

- (void)showOutcome:(NSImageView *)symbol reason:(NSTextField *)reason hold:(NSTimeInterval)hold {
    if (_closed) return;
    if (reason) {
        NSSize pill = layOutPill(symbol, reason);
        [_panel setFrame:(NSRect){indicatorOriginNearCursor(pill), pill} display:NO];
    }

    NSArray<NSView *> *incoming = reason ? @[symbol, reason] : @[symbol];
    for (NSView *view in incoming) {
        view.alphaValue = 0;
        [_badge addSubview:view];
    }
    [self redrawShadow];

    NSProgressIndicator *spinner = _spinner;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = kCrossfade;
        spinner.animator.alphaValue = 0;
        for (NSView *view in incoming) view.animator.alphaValue = 1;
    } completionHandler:^{
        [spinner stopAnimation:nil];
        [spinner removeFromSuperview];
    }];
    if (![[NSWorkspace sharedWorkspace] accessibilityDisplayShouldReduceMotion]) {
        [symbol addSymbolEffect:[NSSymbolBounceEffect effect]];
    }

    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(hold * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        [self fadeOutAndClose];
    });
}

- (void)fadeOutAndClose {
    if (_closed) return;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = kFadeOut;
        self->_panel.animator.alphaValue = 0;
    } completionHandler:^{
        [self close];
    }];
}

- (void)close {
    _closed = YES;
    [_followCursor invalidate];
    [_panel close];
}

@end

@interface ProofreadService : NSObject
- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error;
@end

@implementation ProofreadService

- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    logMsg(@"Service invoked");

    NSString *text = [pasteboard stringForType:NSPasteboardTypeString];
    if (!text || text.length == 0) {
        logMsg(@"No text on pasteboard");
        if (error) *error = @"No text to proofread";
        return;
    }
    logMsg(@"Got text (%lu chars)", (unsigned long)text.length);

    CursorBusyIndicator *busyIndicator = [[CursorBusyIndicator alloc] init];

    NSString *proofdPath = [[NSBundle mainBundle] pathForAuxiliaryExecutable:@"proofd"];
    if (!proofdPath) {
        [busyIndicator finishWithFailure:@"Reinstall proofd"];
        logMsg(@"proofd binary missing from %@", [[NSBundle mainBundle] bundlePath]);
        if (error) *error = @"proofd binary missing from proofd.app; reinstall it";
        return;
    }

    NSPipe *inputPipe = [NSPipe pipe];
    NSPipe *outputPipe = [NSPipe pipe];
    NSPipe *errorPipe = [NSPipe pipe];

    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:proofdPath];
    NSMutableDictionary *environment = [[[NSProcessInfo processInfo] environment] mutableCopy];
    if (!environment[@"RUST_LOG"]) environment[@"RUST_LOG"] = @"info";
    task.environment = environment;
    task.standardInput = inputPipe;
    task.standardOutput = outputPipe;
    task.standardError = errorPipe;

    NSDate *started = [NSDate date];
    NSError *launchError = nil;
    if (![task launchAndReturnError:&launchError]) {
        [busyIndicator finishWithFailure:@"Reinstall proofd"];
        logMsg(@"proofd failed to launch: %@", launchError);
        if (error) *error = [NSString stringWithFormat:@"proofd failed to launch: %@", launchError.localizedDescription];
        return;
    }

    // Written after launch: a selection larger than the pipe buffer would block forever with no reader.
    NSData *textData = [text dataUsingEncoding:NSUTF8StringEncoding];
    NSFileHandle *stdinHandle = inputPipe.fileHandleForWriting;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        [stdinHandle writeData:textData error:nil];
        [stdinHandle closeAndReturnError:nil];
    });

    // Drained while proofd runs: output larger than the pipe buffer (64 KB) would block it until the deadline.
    __block NSData *outputData = nil;
    __block NSData *errData = nil;
    dispatch_group_t drained = dispatch_group_create();
    dispatch_queue_t readQueue = dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0);
    dispatch_group_async(drained, readQueue, ^{
        outputData = [outputPipe.fileHandleForReading readDataToEndOfFile];
    });
    dispatch_group_async(drained, readQueue, ^{
        errData = [errorPipe.fileHandleForReading readDataToEndOfFile];
    });

    NSDate *deadline = [started dateByAddingTimeInterval:servicesTimeout() - kServicesTimeoutMargin];
    if (!waitForExit(task, deadline)) {
        // proofd stops its provider on SIGTERM, but a blocking read or DNS lookup can delay it past the caller's patience.
        [task terminate];
        if (!waitForExit(task, [NSDate dateWithTimeIntervalSinceNow:kTerminateGrace])) {
            kill(task.processIdentifier, SIGKILL);
        }
        [task waitUntilExit];
        [busyIndicator finishWithFailure:@"Timed out"];
        logMsg(@"proofd stopped after %.0fms: the calling app stops waiting at the services timeout",
               -[started timeIntervalSinceNow] * 1000);
        if (error) *error = @"proofd took longer than the services timeout";
        return;
    }
    logMsg(@"proofd exited %ld after %.0fms", (long)[task terminationStatus], -[started timeIntervalSinceNow] * 1000);

    // A process proofd started could inherit the pipes and keep them open after proofd exits.
    int64_t drainBudget = (int64_t)(MAX(deadline.timeIntervalSinceNow, 0) * NSEC_PER_SEC);
    if (dispatch_group_wait(drained, dispatch_time(DISPATCH_TIME_NOW, drainBudget)) != 0) {
        [busyIndicator finishWithFailure:@"Proofread failed"];
        logMsg(@"proofd exited but its output pipes stayed open");
        if (error) *error = @"proofd output never finished";
        return;
    }
    NSString *errMsg = [[NSString alloc] initWithData:errData encoding:NSUTF8StringEncoding];
    if (errMsg.length > 0) {
        logMsg(@"proofd stderr:\n%@", errMsg);
    }

    if ([task terminationStatus] != 0) {
        [busyIndicator finishWithFailure:shortFailureReason(errMsg)];
        if (!errMsg || errMsg.length == 0) errMsg = @"unknown error";
        if (error) {
            *error = [NSString stringWithFormat:@"proofd failed (exit %ld): %@", (long)[task terminationStatus], errMsg];
        }
        return;
    }

    NSString *result = [[NSString alloc] initWithData:outputData encoding:NSUTF8StringEncoding];
    if (!result) {
        [busyIndicator finishWithFailure:@"Proofread failed"];
        logMsg(@"Failed to read proofd output");
        if (error) *error = @"Failed to read proofd output";
        return;
    }
    logMsg(@"Got result (%lu chars)", (unsigned long)result.length);
    [busyIndicator finishWithSuccess];

    [pasteboard clearContents];
    [pasteboard setString:result forType:NSPasteboardTypeString];
    logMsg(@"Wrote result back to pasteboard");
}

@end

@interface AppDelegate : NSObject <NSApplicationDelegate>
@end

@implementation AppDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    logMsg(@"App launching, registering service provider");
    NSRegisterServicesProvider([[ProofreadService alloc] init], @"proofd");
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        // proofd can exit before reading stdin (e.g. bad config); the write must fail, not kill the host.
        signal(SIGPIPE, SIG_IGN);
        NSApplication *app = [NSApplication sharedApplication];
        [app setDelegate:[[AppDelegate alloc] init]];
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [app run];
    }
    return 0;
}
