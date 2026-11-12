#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import "ProofreadService.h"
#import "ProofreadProcess.h"
#import "ResultPanel.h"

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
        [[NSFileManager defaultManager] createDirectoryAtPath:logDir withIntermediateDirectories:YES attributes:nil error:NULL];
        [data writeToFile:logPath atomically:YES];
    } else {
        [fh seekToEndOfFile];
        [fh writeData:data];
        [fh closeFile];
    }
}

@interface ProofreadService ()
@property(nonatomic, copy) NSString *executable;
@property(nonatomic, strong) NSPasteboard *clipboard;
@property(nonatomic, strong) ResultPanel *resultPanel;
@property(nonatomic) NSUInteger invocationGeneration;
@end

@implementation ProofreadService

- (instancetype)initWithExecutable:(NSString *)executable clipboard:(NSPasteboard *)clipboard {
    self = [super init];
    if (self) {
        _executable = [executable copy];
        _clipboard = clipboard;
    }
    return self;
}

- (void)proofreadClipboardService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    NSUInteger invocation = ++self.invocationGeneration;
    NSInteger changeCount = self.clipboard.changeCount;
    [self.resultPanel close];
    logMsg(@"Clipboard service invoked");
    // This action has no send/return types. Read only the general clipboard;
    // the calling app never has to expose a selection or accept replacement text.
    NSString *text = [self.clipboard stringForType:NSPasteboardTypeString];
    NSString *settingsError = nil;
    NSNumber *popup = [self popupSettingWithError:&settingsError];
    if (invocation != self.invocationGeneration) return;
    if (!popup) {
        if (error) *error = settingsError;
        return;
    }
    if (![text stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length) {
        if (popup.boolValue) {
            if (!self.resultPanel) self.resultPanel = [[ResultPanel alloc] initWithPasteboard:self.clipboard];
            [self.resultPanel showMessage:@"Copy some text first"];
        } else {
            if (error) *error = @"Copy some text first";
            logMsg(@"Clipboard contains no text");
        }
        return;
    }
    if (!self.resultPanel) self.resultPanel = [[ResultPanel alloc] initWithPasteboard:self.clipboard];
    [self.resultPanel beginProofreading:text executable:self.executable clipboardChangeCount:changeCount showPopup:popup.boolValue];
}

- (NSNumber *)popupSettingWithError:(NSString **)error {
    ProofreadProcessResult *settingsResult = ProofreadRunProcess(self.executable, @[@"--service-settings"], @"");
    if (settingsResult.error) {
        logMsg(@"Settings failed: %@", settingsResult.error);
        if (error) *error = settingsResult.error;
        return nil;
    }
    id settings = [NSJSONSerialization JSONObjectWithData:[settingsResult.output dataUsingEncoding:NSUTF8StringEncoding]
                                                options:0 error:NULL];
    id popup = [settings isKindOfClass:NSDictionary.class] ? settings[@"show_result_popup"] : nil;
    if (!popup || CFGetTypeID((__bridge CFTypeRef)popup) != CFBooleanGetTypeID()) {
        if (error) *error = @"Invalid service settings from proofd; reinstall the app";
        return nil;
    }
    return popup;
}

- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    NSUInteger invocation = ++self.invocationGeneration;
    NSInteger clipboardChangeCount = self.clipboard.changeCount;
    // Invalidate before settings lookup: NSTask can pump the main run loop
    // while waiting, and invalid config must also retire earlier requests.
    [self.resultPanel close];
    logMsg(@"Service invoked");

    NSString *text = [pasteboard stringForType:NSPasteboardTypeString];
    if (!text || text.length == 0) {
        logMsg(@"No text on pasteboard");
        if (error) *error = @"No text to proofread";
        return;
    }
    logMsg(@"Got selection (%lu UTF-16 units)", (unsigned long)text.length);

    NSString *settingsError = nil;
    NSNumber *popup = [self popupSettingWithError:&settingsError];
    if (invocation != self.invocationGeneration) {
        [pasteboard clearContents];
        return;
    }
    if (!popup) {
        if (error) *error = settingsError;
        return;
    }

    if ([popup boolValue]) {
        if (!self.resultPanel) self.resultPanel = [[ResultPanel alloc] initWithPasteboard:self.clipboard];
        // The service advertises a return type for its default replacement mode.
        // Remove it here: returning the original string would reinsert it and
        // collapse the caller's selection; returning "" would delete it.
        [pasteboard clearContents];
        [self.resultPanel beginProofreading:text executable:self.executable clipboardChangeCount:clipboardChangeCount];
        return;
    }

    NSStatusItem *busyIndicator = [[NSStatusBar systemStatusBar] statusItemWithLength:NSSquareStatusItemLength];
    busyIndicator.button.image = [NSImage imageWithSystemSymbolName:@"text.magnifyingglass"
                                           accessibilityDescription:@"Proofreading"];

    NSDate *started = [NSDate date];
    ProofreadProcessResult *process = ProofreadRunProcess(self.executable, @[], text);
    [[NSStatusBar systemStatusBar] removeStatusItem:busyIndicator];
    if (invocation != self.invocationGeneration) {
        [pasteboard clearContents];
        return;
    }
    logMsg(@"proofd finished after %.0fms", -[started timeIntervalSinceNow] * 1000);
    if (process.standardError.length > 0) {
        logMsg(@"proofd stderr:\n%@", process.standardError);
    }
    if (process.error) {
        if (error) *error = process.error;
        return;
    }
    if (!process.output.length) {
        if (error) *error = @"proofd returned empty output";
        return;
    }

    [pasteboard clearContents];
    [pasteboard setString:process.output forType:NSPasteboardTypeString];
    logMsg(@"Wrote result back to pasteboard");
}

@end

#ifndef PROOFD_NO_MAIN
@interface AppDelegate : NSObject <NSApplicationDelegate>
@property(nonatomic, strong) ProofreadService *service;
@end

@implementation AppDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    logMsg(@"App launching, registering service provider");
    NSString *executable = [[NSBundle mainBundle] pathForAuxiliaryExecutable:@"proofd"];
    self.service = [[ProofreadService alloc] initWithExecutable:executable clipboard:NSPasteboard.generalPasteboard];
    NSRegisterServicesProvider(self.service, @"proofd");
    // Stay running; service invocations are handled while app runs
    // App is terminated by user or system when no longer needed
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        AppDelegate *delegate = [[AppDelegate alloc] init];
        [app setDelegate:delegate];
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [app run];
    }
    return 0;
}
#endif
