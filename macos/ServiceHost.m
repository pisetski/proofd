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
    logMsg(@"Got text: %@", [text substringToIndex:MIN(text.length, 50)]);

    NSString *proofdPath = [[NSBundle mainBundle] pathForAuxiliaryExecutable:@"proofd"];
    if (!proofdPath) {
        logMsg(@"proofd binary missing from %@", [[NSBundle mainBundle] bundlePath]);
        if (error) *error = @"proofd binary missing from proofd.app; reinstall it";
        return;
    }

    NSPipe *inputPipe = [NSPipe pipe];
    NSPipe *outputPipe = [NSPipe pipe];
    NSPipe *errorPipe = [NSPipe pipe];

    NSTask *task = [[NSTask alloc] init];
    [task setLaunchPath:proofdPath];
        [task setStandardInput:inputPipe];
    [task setStandardOutput:outputPipe];
    [task setStandardError:errorPipe];

    NSData *textData = [text dataUsingEncoding:NSUTF8StringEncoding];
    [[inputPipe fileHandleForWriting] writeData:textData];
    [[inputPipe fileHandleForWriting] closeFile];

    NSDate *started = [NSDate date];
    [task launch];
    [task waitUntilExit];
    logMsg(@"proofd exited %ld after %.0fms", (long)[task terminationStatus], -[started timeIntervalSinceNow] * 1000);

    NSData *errData = [[errorPipe fileHandleForReading] readDataToEndOfFile];
    NSString *errMsg = [[NSString alloc] initWithData:errData encoding:NSUTF8StringEncoding];
    if (errMsg.length > 0) {
        logMsg(@"proofd stderr:\n%@", errMsg);
    }

    if ([task terminationStatus] != 0) {
        if (!errMsg || errMsg.length == 0) errMsg = @"unknown error";
        if (error) {
            *error = [NSString stringWithFormat:@"proofd failed (exit %ld): %@", (long)[task terminationStatus], errMsg];
        }
        return;
    }

    NSData *outputData = [[outputPipe fileHandleForReading] readDataToEndOfFile];
    NSString *result = [[NSString alloc] initWithData:outputData encoding:NSUTF8StringEncoding];
    if (!result) {
        logMsg(@"Failed to read proofd output");
        if (error) *error = @"Failed to read proofd output";
        return;
    }
    logMsg(@"Got result: %@", [result substringToIndex:MIN(result.length, 50)]);

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
    // Stay running; service invocations are handled while app runs
    // App is terminated by user or system when no longer needed
}
@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        [app setDelegate:[[AppDelegate alloc] init]];
        [app setActivationPolicy:NSApplicationActivationPolicyProhibited];
        [app run];
    }
    return 0;
}
