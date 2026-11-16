#import "BusyIndicator.h"

static const CGFloat kLegacyIndicatorSize = 30;
static const CGFloat kGlassIndicatorSize = 34;
static const NSInteger kGlassStyleRegular = 0;
static const NSInteger kGlassStyleClear = 1;
static const CGFloat kIndicatorCursorOffset = 14;
static const CGFloat kPillPadding = 10;
static const CGFloat kPillSymbolGap = 5;
static const CGFloat kBadgeRimWidth = 0.5;
static const CGFloat kOutcomeSymbolPointSize = 13;
static const CGFloat kReasonFontSize = 12;
static const NSTimeInterval kCursorPollInterval = 1.0 / 60;
static const NSTimeInterval kFadeIn = 0.15;
static const NSTimeInterval kCrossfade = 0.12;
static const NSTimeInterval kFadeOut = 0.25;
static const NSTimeInterval kSuccessHold = 0.8;
static const NSTimeInterval kFailureHold = 2.5;

static BOOL glassAvailable(void) {
    return NSClassFromString(@"NSGlassEffectView") != nil;
}

static CGFloat indicatorSize(void) {
    return glassAvailable() ? kGlassIndicatorSize : kLegacyIndicatorSize;
}

static NSPoint clampedIntoScreenUnderCursor(NSPoint origin, NSSize size, NSPoint cursor) {
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

static NSPoint indicatorOriginNearCursor(NSSize size) {
    NSPoint cursor = [NSEvent mouseLocation];
    NSPoint belowRightOfCursor = NSMakePoint(cursor.x + kIndicatorCursorOffset,
                                             cursor.y - kIndicatorCursorOffset - size.height);
    return clampedIntoScreenUnderCursor(belowRightOfCursor, size, cursor);
}

static void centerInLeadingSquare(NSView *view) {
    [view setFrameOrigin:NSMakePoint((indicatorSize() - NSWidth(view.frame)) / 2,
                                     (indicatorSize() - NSHeight(view.frame)) / 2)];
}

static NSPanel *makeIndicatorPanel(void) {
    NSSize size = NSMakeSize(indicatorSize(), indicatorSize());
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
@property (nonatomic) BOOL drawsOwnBackground;
@end

@implementation BadgeView

- (BOOL)isOpaque {
    return NO;
}

- (void)drawRect:(NSRect)dirtyRect {
    if (!self.drawsOwnBackground) return;
    NSRect shape = NSInsetRect(self.bounds, kBadgeRimWidth / 2, kBadgeRimWidth / 2);
    CGFloat radius = NSHeight(shape) / 2;
    NSBezierPath *capsule = [NSBezierPath bezierPathWithRoundedRect:shape xRadius:radius yRadius:radius];
    [appearanceColor([NSColor whiteColor], [NSColor colorWithWhite:0.2 alpha:1]) setFill];
    [capsule fill];
    capsule.lineWidth = kBadgeRimWidth;
    [appearanceColor([NSColor colorWithWhite:0 alpha:0.12], [NSColor colorWithWhite:1 alpha:0.16]) setStroke];
    [capsule stroke];
}

@end

static void setGlassStyle(NSView *glass, NSInteger style) {
    [glass setValue:@(style) forKey:@"style"];
}

// Looked up by name so the host still builds with SDKs older than macOS 26.
static NSView *makeGlassHosting(NSView *content) {
    Class glassClass = NSClassFromString(@"NSGlassEffectView");
    if (!glassClass) return nil;
    NSView *glass = [[glassClass alloc] initWithFrame:content.frame];
    [glass setValue:@(indicatorSize() / 2) forKey:@"cornerRadius"];
    [glass setValue:content forKey:@"contentView"];
    setGlassStyle(glass, kGlassStyleClear);
    return glass;
}

static NSProgressIndicator *makeSpinner(void) {
    NSProgressIndicator *spinner = [[NSProgressIndicator alloc] init];
    spinner.style = NSProgressIndicatorStyleSpinning;
    spinner.controlSize = NSControlSizeSmall;
    spinner.usesThreadedAnimation = YES;
    [spinner sizeToFit];
    centerInLeadingSquare(spinner);
    return spinner;
}

static NSImageView *makeOutcomeSymbol(BOOL success) {
    NSString *name = success ? @"checkmark" : @"exclamationmark.triangle.fill";
    NSString *description = success ? @"Proofread" : @"Proofreading failed";
    NSImageView *symbol = [NSImageView imageViewWithImage:[NSImage imageWithSystemSymbolName:name
                                                                     accessibilityDescription:description]];
    symbol.symbolConfiguration = [NSImageSymbolConfiguration configurationWithPointSize:kOutcomeSymbolPointSize
                                                                                 weight:NSFontWeightSemibold];
    symbol.contentTintColor = success ? [NSColor systemGreenColor] : [NSColor systemOrangeColor];
    [symbol sizeToFit];
    centerInLeadingSquare(symbol);
    return symbol;
}

static NSTextField *makeReasonLabel(NSString *reason) {
    NSTextField *label = [NSTextField labelWithString:reason];
    label.font = [NSFont systemFontOfSize:kReasonFontSize weight:NSFontWeightMedium];
    label.textColor = [NSColor labelColor];
    [label sizeToFit];
    return label;
}

static NSSize layOutPill(NSImageView *symbol, NSTextField *label) {
    CGFloat labelTextInset = (NSWidth(label.frame) - ceil(label.attributedStringValue.size.width)) / 2;
    [symbol setFrameOrigin:NSMakePoint(kPillPadding, NSMinY(symbol.frame))];
    [label setFrameOrigin:NSMakePoint(NSMaxX(symbol.frame) + kPillSymbolGap - labelTextInset,
                                      (indicatorSize() - NSHeight(label.frame)) / 2)];
    return NSMakeSize(NSMaxX(label.frame) - labelTextInset + kPillPadding, indicatorSize());
}

static BOOL shouldAnimateMotion(void) {
    return ![[NSWorkspace sharedWorkspace] accessibilityDisplayShouldReduceMotion];
}

@implementation CursorBusyIndicator {
    NSPanel *_panel;
    NSView *_glass;
    BadgeView *_badge;
    NSProgressIndicator *_spinner;
    NSTimer *_cursorPoll;
    BOOL _closed;
}

static __weak CursorBusyIndicator *visibleIndicator;

- (instancetype)init {
    if (!(self = [super init])) return nil;
    [visibleIndicator close];
    visibleIndicator = self;
    _panel = makeIndicatorPanel();
    _badge = [[BadgeView alloc] initWithFrame:NSMakeRect(0, 0, indicatorSize(), indicatorSize())];
    _badge.wantsLayer = YES;
    _spinner = makeSpinner();
    [_badge addSubview:_spinner];
    _glass = makeGlassHosting(_badge);
    _badge.drawsOwnBackground = _glass == nil;
    _panel.contentView = _glass ?: _badge;
    [_spinner startAnimation:nil];
    [self redisplayAndRecomputeShadow];
    [self fadeIn];
    [self startFollowingCursor];
    return self;
}

- (void)fadeIn {
    _panel.alphaValue = 0;
    [_panel orderFrontRegardless];
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = kFadeIn;
        self->_panel.animator.alphaValue = 1;
    }];
}

// Polled because while proofd runs the run loop spins but never dispatches mouse-moved events.
- (void)startFollowingCursor {
    NSPanel *panel = _panel;
    _cursorPoll = [NSTimer scheduledTimerWithTimeInterval:kCursorPollInterval
                                                  repeats:YES
                                                    block:^(NSTimer *timer) {
        NSPoint origin = indicatorOriginNearCursor(panel.frame.size);
        if (!NSEqualPoints(origin, panel.frame.origin)) [panel setFrameOrigin:origin];
    }];
}

- (void)redisplayAndRecomputeShadow {
    [_panel display];
    [_panel invalidateShadow];
}

- (void)finishWithSuccess {
    [self showOutcome:makeOutcomeSymbol(YES) reason:nil holdFor:kSuccessHold];
}

- (void)finishWithFailure:(NSString *)reason {
    [self showOutcome:makeOutcomeSymbol(NO) reason:makeReasonLabel(reason) holdFor:kFailureHold];
}

- (void)showOutcome:(NSImageView *)symbol reason:(nullable NSTextField *)reason holdFor:(NSTimeInterval)hold {
    if (_closed) return;
    if (reason) {
        if (_glass) setGlassStyle(_glass, kGlassStyleRegular);
        [self growIntoPillHolding:symbol reason:reason];
    }

    NSArray<NSView *> *incoming = reason ? @[symbol, reason] : @[symbol];
    for (NSView *view in incoming) {
        view.alphaValue = 0;
        [_badge addSubview:view];
    }
    [self redisplayAndRecomputeShadow];
    [self crossfadeSpinnerInto:incoming];
    if (shouldAnimateMotion()) [symbol addSymbolEffect:[NSSymbolBounceEffect effect]];

    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(hold * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        [self fadeOutAndClose];
    });
}

- (void)growIntoPillHolding:(NSImageView *)symbol reason:(NSTextField *)reason {
    NSSize pill = layOutPill(symbol, reason);
    [_panel setFrame:(NSRect){indicatorOriginNearCursor(pill), pill} display:NO];
}

- (void)crossfadeSpinnerInto:(NSArray<NSView *> *)incoming {
    NSProgressIndicator *spinner = _spinner;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = kCrossfade;
        spinner.animator.alphaValue = 0;
        for (NSView *view in incoming) view.animator.alphaValue = 1;
    } completionHandler:^{
        [spinner stopAnimation:nil];
        [spinner removeFromSuperview];
    }];
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
    [_cursorPoll invalidate];
    [_panel close];
}

@end
