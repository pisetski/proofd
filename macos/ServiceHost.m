#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import "BusyIndicator.h"
#import "Restyle.h"
#import "Selection.h"
#import "ServiceLog.h"

static const NSTimeInterval kDefaultServicesTimeout = 30;
static const NSTimeInterval kServicesTimeoutMargin = 1;
static const NSTimeInterval kTerminateGrace = 0.5;
static const NSTimeInterval kRunLoopSlice = 0.05;
static NSString *const kGenericFailureReason = @"Proofread failed";
static NSString *const kProofdErrorPrefix = @"proofd: ";

static NSTimeInterval servicesTimeoutFromInfoPlist(void) {
    NSArray *services = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSServices"];
    id milliseconds = [services.firstObject objectForKey:@"NSTimeout"];
    return milliseconds ? [milliseconds doubleValue] / 1000 : kDefaultServicesTimeout;
}

static double millisecondsSince(NSDate *start) {
    return -[start timeIntervalSinceNow] * 1000;
}

static NSString *lastProofdErrorMessage(NSString *proofdStderr) {
    NSString *message = @"";
    for (NSString *line in [proofdStderr componentsSeparatedByCharactersInSet:[NSCharacterSet newlineCharacterSet]]) {
        if ([line hasPrefix:kProofdErrorPrefix]) message = [line substringFromIndex:kProofdErrorPrefix.length];
    }
    return message;
}

static NSString *pillReasonForProofdError(NSString *proofdStderr) {
    NSString *message = lastProofdErrorMessage(proofdStderr);
    // Prefixes only: the rest of a line can quote the provider's output. tests/proofread_cli.rs parses this table.
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
    ];
    for (NSArray<NSString *> *reason in reasons) {
        if ([message hasPrefix:reason[0]]) return reason[1];
    }
    return kGenericFailureReason;
}

@interface ProofreadFailure : NSObject
@property (readonly) NSString *pillReason;
@property (readonly) NSString *serviceError;
@end

@implementation ProofreadFailure

+ (instancetype)failureWithPillReason:(NSString *)pillReason serviceError:(NSString *)serviceError {
    ProofreadFailure *failure = [[self alloc] init];
    failure->_pillReason = pillReason;
    failure->_serviceError = serviceError;
    return failure;
}

@end

static NSDictionary<NSString *, NSString *> *environmentLoggingAtInfo(void) {
    NSMutableDictionary *environment = [[[NSProcessInfo processInfo] environment] mutableCopy];
    if (!environment[@"RUST_LOG"]) environment[@"RUST_LOG"] = @"info";
    return environment;
}

@interface ProofdProcess : NSObject
@property (readonly, nullable) NSString *outputText;
@property (readonly, nullable) NSString *errorText;
@property (readonly) long exitStatus;
@end

@implementation ProofdProcess {
    NSTask *_task;
    NSPipe *_inputPipe;
    NSPipe *_outputPipe;
    NSPipe *_errorPipe;
    dispatch_group_t _outputDrained;
    NSData *_outputData;
    NSData *_errorData;
}

- (instancetype)initWithExecutable:(NSString *)path {
    if (!(self = [super init])) return nil;
    _inputPipe = [NSPipe pipe];
    _outputPipe = [NSPipe pipe];
    _errorPipe = [NSPipe pipe];
    _outputDrained = dispatch_group_create();
    _task = [[NSTask alloc] init];
    _task.executableURL = [NSURL fileURLWithPath:path];
    _task.environment = environmentLoggingAtInfo();
    _task.standardInput = _inputPipe;
    _task.standardOutput = _outputPipe;
    _task.standardError = _errorPipe;
    return self;
}

- (BOOL)launchWithInput:(NSString *)text error:(NSError **)error {
    if (![_task launchAndReturnError:error]) return NO;
    [self writeInputInBackground:[text dataUsingEncoding:NSUTF8StringEncoding]];
    [self drainOutputInBackground];
    return YES;
}

// After launch and off the main thread: input larger than the pipe buffer blocks until proofd reads it.
- (void)writeInputInBackground:(NSData *)input {
    NSFileHandle *stdinHandle = _inputPipe.fileHandleForWriting;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        [stdinHandle writeData:input error:nil];
        [stdinHandle closeAndReturnError:nil];
    });
}

// Undrained, output larger than the pipe buffer (64 KB) would block proofd until the deadline.
- (void)drainOutputInBackground {
    dispatch_queue_t readQueue = dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0);
    NSFileHandle *output = _outputPipe.fileHandleForReading, *errors = _errorPipe.fileHandleForReading;
    dispatch_group_async(_outputDrained, readQueue, ^{
        self->_outputData = [output readDataToEndOfFile];
    });
    dispatch_group_async(_outputDrained, readQueue, ^{
        self->_errorData = [errors readDataToEndOfFile];
    });
}

- (BOOL)spinRunLoopUntilExitOr:(NSDate *)deadline {
    while (_task.isRunning && deadline.timeIntervalSinceNow > 0) {
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:kRunLoopSlice]];
    }
    return !_task.isRunning;
}

- (void)terminateThenKillAfterGrace {
    [_task terminate];
    if (![self spinRunLoopUntilExitOr:[NSDate dateWithTimeIntervalSinceNow:kTerminateGrace]]) {
        kill(_task.processIdentifier, SIGKILL);
    }
    [_task waitUntilExit];
}

// A process proofd started can inherit the pipes and hold them open after proofd exits.
- (BOOL)waitForOutputPipesToCloseBy:(NSDate *)deadline {
    int64_t budget = (int64_t)(MAX(deadline.timeIntervalSinceNow, 0) * NSEC_PER_SEC);
    return dispatch_group_wait(_outputDrained, dispatch_time(DISPATCH_TIME_NOW, budget)) == 0;
}

- (long)exitStatus {
    return _task.terminationStatus;
}

- (NSString *)outputText {
    return [[NSString alloc] initWithData:_outputData encoding:NSUTF8StringEncoding];
}

- (NSString *)errorText {
    return [[NSString alloc] initWithData:_errorData encoding:NSUTF8StringEncoding];
}

@end

static NSString *revisedByBundledProofd(NSString *text, ProofreadFailure **failure) {
    NSString *proofdPath = [[NSBundle mainBundle] pathForAuxiliaryExecutable:@"proofd"];
    if (!proofdPath) {
        serviceLog(@"proofd binary missing from %@", [[NSBundle mainBundle] bundlePath]);
        *failure = [ProofreadFailure failureWithPillReason:@"Reinstall proofd"
                                       serviceError:@"proofd binary missing from proofd.app; reinstall it"];
        return nil;
    }

    ProofdProcess *proofd = [[ProofdProcess alloc] initWithExecutable:proofdPath];
    NSDate *started = [NSDate date];
    NSDate *deadline = [started dateByAddingTimeInterval:servicesTimeoutFromInfoPlist() - kServicesTimeoutMargin];
    NSError *launchError = nil;
    if (![proofd launchWithInput:text error:&launchError]) {
        serviceLog(@"proofd failed to launch: %@", launchError);
        NSString *serviceError = [NSString stringWithFormat:@"proofd failed to launch: %@", launchError.localizedDescription];
        *failure = [ProofreadFailure failureWithPillReason:@"Reinstall proofd" serviceError:serviceError];
        return nil;
    }

    if (![proofd spinRunLoopUntilExitOr:deadline]) {
        [proofd terminateThenKillAfterGrace];
        serviceLog(@"proofd stopped after %.0fms: the calling app stops waiting at the services timeout",
                   millisecondsSince(started));
        *failure = [ProofreadFailure failureWithPillReason:@"Timed out" serviceError:@"proofd took longer than the services timeout"];
        return nil;
    }
    serviceLog(@"proofd exited %ld after %.0fms", proofd.exitStatus, millisecondsSince(started));

    if (![proofd waitForOutputPipesToCloseBy:deadline]) {
        serviceLog(@"proofd exited but its output pipes stayed open");
        *failure = [ProofreadFailure failureWithPillReason:kGenericFailureReason serviceError:@"proofd output never finished"];
        return nil;
    }

    NSString *errorText = proofd.errorText;
    if (errorText.length > 0) serviceLog(@"proofd stderr:\n%@", errorText);
    if (proofd.exitStatus != 0) {
        NSString *detail = errorText.length > 0 ? errorText : @"unknown error";
        NSString *serviceError = [NSString stringWithFormat:@"proofd failed (exit %ld): %@", proofd.exitStatus, detail];
        *failure = [ProofreadFailure failureWithPillReason:pillReasonForProofdError(errorText) serviceError:serviceError];
        return nil;
    }

    NSString *revised = proofd.outputText;
    if (!revised) {
        serviceLog(@"Failed to read proofd output");
        *failure = [ProofreadFailure failureWithPillReason:kGenericFailureReason serviceError:@"Failed to read proofd output"];
    }
    return revised;
}

static Selection *selectionOnPasteboard(NSPasteboard *pasteboard) {
    NSData *rtf = [pasteboard dataForType:NSPasteboardTypeRTF];
    NSAttributedString *richCopy = rtf ? [[NSAttributedString alloc] initWithRTF:rtf documentAttributes:nil] : nil;
    Selection *selection = [Selection selectionWithPlainCopy:[pasteboard stringForType:NSPasteboardTypeString]
                                                    richCopy:richCopy];
    if (selection.text.length == 0) return nil;
    serviceLog(@"Got %@ text (%lu chars, %lu list bullets left out)", selection.styledText ? @"rich" : @"plain",
               (unsigned long)selection.text.length, (unsigned long)selection.bulletsLeftOut);
    if (richCopy && !selection.styledText) serviceLog(@"Rich copy differs from the selection; proofreading it as plain text");
    return selection;
}

static NSData *rtfOfRevisionStyledLike(NSAttributedString *styledOriginal, NSString *revised) {
    NSAttributedString *restyled = revisedTextStyledLike(styledOriginal, revised);
    return [restyled RTFFromRange:NSMakeRange(0, restyled.length)
               documentAttributes:@{NSDocumentTypeDocumentAttribute: NSRTFTextDocumentType}];
}

static void writeRevision(NSPasteboard *pasteboard, NSString *revised, NSAttributedString *styledOriginal) {
    NSData *restyledRTF = styledOriginal ? rtfOfRevisionStyledLike(styledOriginal, revised) : nil;
    if (restyledRTF) {
        [pasteboard declareTypes:@[NSPasteboardTypeRTF, NSPasteboardTypeString] owner:nil];
        [pasteboard setData:restyledRTF forType:NSPasteboardTypeRTF];
    } else {
        [pasteboard declareTypes:@[NSPasteboardTypeString] owner:nil];
    }
    [pasteboard setString:revised forType:NSPasteboardTypeString];
}

@interface ProofreadService : NSObject
- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error;
@end

@implementation ProofreadService

- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    serviceLog(@"Service invoked");

    Selection *selection = selectionOnPasteboard(pasteboard);
    if (!selection) {
        serviceLog(@"No text on pasteboard");
        if (error) *error = @"No text to proofread";
        return;
    }

    CursorBusyIndicator *busyIndicator = [[CursorBusyIndicator alloc] init];
    ProofreadFailure *failure = nil;
    NSString *revised = revisedByBundledProofd(selection.text, &failure);
    if (!revised) {
        [busyIndicator finishWithFailure:failure.pillReason];
        if (error) *error = failure.serviceError;
        return;
    }
    serviceLog(@"Got result (%lu chars)", (unsigned long)revised.length);
    [busyIndicator finishWithSuccess];

    writeRevision(pasteboard, revised, selection.styledText);
    serviceLog(@"Wrote result back to pasteboard");
}

@end

@interface AppDelegate : NSObject <NSApplicationDelegate>
@end

@implementation AppDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    serviceLog(@"App launching, registering service provider");
    NSRegisterServicesProvider([[ProofreadService alloc] init], @"proofd");
}
@end

static void surviveWritesToAProofdThatExitedEarly(void) {
    signal(SIGPIPE, SIG_IGN);
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        surviveWritesToAProofdThatExitedEarly();
        NSApplication *app = [NSApplication sharedApplication];
        [app setDelegate:[[AppDelegate alloc] init]];
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [app run];
    }
    return 0;
}
