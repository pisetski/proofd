#import "ResultPanel.h"
#import "ProofreadProcess.h"

@interface ResultPanel ()
@property(nonatomic, strong) NSPasteboard *pasteboard;
@property(nonatomic, strong) NSTextField *statusLabel;
@property(nonatomic, strong) NSProgressIndicator *spinner;
@property(nonatomic, strong) NSScrollView *scrollView;
@property(nonatomic, strong) NSTextView *resultView;
@property(nonatomic, strong) NSButton *actionButton;
@property(nonatomic, copy) NSString *input;
@property(nonatomic, copy) NSString *executable;
@property(nonatomic, copy) NSString *result;
@property(nonatomic) NSUInteger generation;
@property(nonatomic) BOOL busy;
@property(nonatomic, strong) id globalMouseMonitor;
@property(nonatomic, strong) id localMouseMonitor;
@end

@implementation ResultPanel

- (instancetype)initWithPasteboard:(NSPasteboard *)pasteboard {
    NSPanel *window = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 420, 70)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskNonactivatingPanel
        backing:NSBackingStoreBuffered defer:NO];
    self = [super initWithWindow:window];
    if (!self) return nil;
    _pasteboard = pasteboard;
    window.title = @"Proofread";
    window.floatingPanel = YES;
    window.hidesOnDeactivate = NO;
    window.becomesKeyOnlyIfNeeded = YES;
    window.collectionBehavior = NSWindowCollectionBehaviorMoveToActiveSpace | NSWindowCollectionBehaviorFullScreenAuxiliary;
    window.releasedWhenClosed = NO;
    window.delegate = self;

    _statusLabel = [NSTextField labelWithString:@""];
    _statusLabel.font = [NSFont systemFontOfSize:12];
    _statusLabel.textColor = NSColor.secondaryLabelColor;
    _statusLabel.lineBreakMode = NSLineBreakByTruncatingTail;
    [window.contentView addSubview:_statusLabel];

    _spinner = [[NSProgressIndicator alloc] initWithFrame:NSMakeRect(16, 25, 16, 16)];
    _spinner.style = NSProgressIndicatorStyleSpinning;
    _spinner.controlSize = NSControlSizeSmall;
    _spinner.displayedWhenStopped = NO;
    [_spinner setAccessibilityLabel:@"Proofreading"];
    [window.contentView addSubview:_spinner];

    _resultView = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 388, 80)];
    _resultView.editable = NO;
    _resultView.richText = NO;
    _resultView.font = [NSFont systemFontOfSize:14];
    _resultView.textColor = NSColor.labelColor;
    _resultView.drawsBackground = NO;
    _resultView.textContainerInset = NSMakeSize(0, 4);
    _resultView.textContainer.lineFragmentPadding = 0;
    _resultView.textContainer.widthTracksTextView = YES;
    _resultView.verticallyResizable = YES;
    _resultView.horizontallyResizable = NO;
    _resultView.autoresizingMask = NSViewWidthSizable;
    [_resultView setAccessibilityLabel:@"Proofread result"];
    _scrollView = [[NSScrollView alloc] initWithFrame:NSZeroRect];
    _scrollView.drawsBackground = NO;
    _scrollView.hasVerticalScroller = YES;
    _scrollView.autohidesScrollers = YES;
    _scrollView.documentView = _resultView;
    [window.contentView addSubview:_scrollView];

    _actionButton = [NSButton buttonWithTitle:@"Copy" target:self action:@selector(copyResult:)];
    _actionButton.bezelStyle = NSBezelStyleRounded;
    [window.contentView addSubview:_actionButton];
    return self;
}

- (void)layoutWithText:(NSString *)text loading:(BOOL)loading {
    NSRect screen = NSScreen.screens.firstObject.visibleFrame;
    CGFloat width = MIN(420, NSWidth(screen) - 32);
    CGFloat height = 70;
    if (text.length) {
        NSRect bounds = [text boundingRectWithSize:NSMakeSize(width - 48, CGFLOAT_MAX)
            options:NSStringDrawingUsesLineFragmentOrigin | NSStringDrawingUsesFontLeading
            attributes:@{NSFontAttributeName: self.resultView.font}];
        height = MIN(360, MAX(140, ceil(NSHeight(bounds)) + 90));
    }
    height = MIN(height, NSHeight(screen) - 64);
    [self.window setContentSize:NSMakeSize(width, height)];
    NSRect frame = self.window.frame;
    frame.origin = NSMakePoint(NSMaxX(screen) - NSWidth(frame) - 16, NSMaxY(screen) - NSHeight(frame) - 16);
    [self.window setFrame:frame display:YES];
    self.scrollView.frame = NSMakeRect(16, 58, width - 32, MAX(0, height - 74));
    self.scrollView.hidden = !text.length;
    // The scroll view starts at zero size. Pin the document to its viewport
    // instead of letting its initial width be added by autoresizing.
    [self.resultView setFrameSize:NSMakeSize(NSWidth(self.scrollView.contentView.bounds),
                                           MAX(80, NSHeight(self.scrollView.contentView.bounds)))];
    self.resultView.string = text ?: @"";
    [self.resultView scrollRangeToVisible:NSMakeRange(0, 0)];
    self.statusLabel.frame = NSMakeRect(loading ? 40 : 16, 23, width - (loading ? 56 : 116), 20);
    self.actionButton.frame = NSMakeRect(width - 94, 16, 80, 32);
    self.actionButton.hidden = loading;
}

- (void)beginProofreading:(NSString *)input executable:(NSString *)executable {
    [self beginProofreading:input executable:executable clipboardChangeCount:self.pasteboard.changeCount];
}

- (void)beginProofreading:(NSString *)input executable:(NSString *)executable clipboardChangeCount:(NSInteger)changeCount {
    self.input = input;
    self.executable = executable;
    self.result = nil;
    self.busy = YES;
    NSUInteger generation = ++self.generation;
    self.statusLabel.stringValue = @"Proofreading…";
    [self layoutWithText:nil loading:YES];
    [self.spinner startAnimation:nil];
    [self.window orderFrontRegardless];
    [self monitorOutsideClicks];

    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        ProofreadProcessResult *process = ProofreadRunProcess(executable, @[], input);
        dispatch_async(dispatch_get_main_queue(), ^{
            // Closing and newer invocations revoke this request's UI/clipboard writes.
            if (generation != self.generation) return;
            self.busy = NO;
            [self.spinner stopAnimation:nil];
            if (process.error || ![process.output stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length) {
                NSLog(@"proofd popup: %@", process.error ?: @"Empty output");
                self.statusLabel.stringValue = @"Couldn’t proofread";
                [self layoutWithText:@"Check your provider and configuration, then try again." loading:NO];
                self.actionButton.title = @"Retry";
                self.actionButton.action = @selector(retry:);
                return;
            }
            self.result = process.output;
            [self layoutWithText:self.result loading:NO];
            self.actionButton.title = @"Copy";
            self.actionButton.action = @selector(copyResult:);
            if ([self.result isEqualToString:input]) {
                self.statusLabel.stringValue = @"No changes needed";
            } else if (self.pasteboard.changeCount == changeCount) {
                [self copyResult:nil];
            } else {
                self.statusLabel.stringValue = @"Ready — Copy to paste";
            }
        });
    });
}

- (void)copyResult:(id)sender {
    if (!self.result.length || self.busy) return;
    [self.pasteboard clearContents];
    BOOL copied = [self.pasteboard setString:self.result forType:NSPasteboardTypeString];
    self.statusLabel.stringValue = copied ? @"Copied — ⌘V to paste" : @"Couldn’t copy — try again";
}

- (void)retry:(id)sender {
    [self beginProofreading:self.input executable:self.executable];
}

- (void)stopMonitoringClicks {
    if (self.globalMouseMonitor) [NSEvent removeMonitor:self.globalMouseMonitor];
    if (self.localMouseMonitor) [NSEvent removeMonitor:self.localMouseMonitor];
    self.globalMouseMonitor = nil;
    self.localMouseMonitor = nil;
}

- (void)monitorOutsideClicks {
    [self stopMonitoringClicks];
    NSEventMask clicks = NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown | NSEventMaskOtherMouseDown;
    __weak ResultPanel *weakSelf = self;
    // Global mouse monitoring needs no Accessibility access. Observe only;
    // the click still reaches the destination app normally.
    self.globalMouseMonitor = [NSEvent addGlobalMonitorForEventsMatchingMask:clicks handler:^(NSEvent *event) {
        [weakSelf close];
    }];
    // Global monitors exclude this app's events. Keep clicks in the popup,
    // including Copy and scrolling, and dismiss on clicks in other windows.
    self.localMouseMonitor = [NSEvent addLocalMonitorForEventsMatchingMask:clicks handler:^NSEvent *(NSEvent *event) {
        ResultPanel *panel = weakSelf;
        if (panel && event.window != panel.window) [panel close];
        return event;
    }];
}

- (void)windowWillClose:(NSNotification *)notification {
    [self stopMonitoringClicks];
    ++self.generation;
    self.busy = NO;
    [self.spinner stopAnimation:nil];
    self.result = nil;
    self.input = nil;
}

- (void)dealloc {
    if (_globalMouseMonitor) [NSEvent removeMonitor:_globalMouseMonitor];
    if (_localMouseMonitor) [NSEvent removeMonitor:_localMouseMonitor];
}

@end
