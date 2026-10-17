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

    NSString *proofdPath = [self findProofd];
    if (!proofdPath) {
        logMsg(@"proofd binary not found");
        if (error) *error = @"proofd binary not found";
        return;
    }
    logMsg(@"Using proofd at: %@", proofdPath);

    NSPipe *inputPipe = [NSPipe pipe];
    NSPipe *outputPipe = [NSPipe pipe];
    NSPipe *errorPipe = [NSPipe pipe];

    NSTask *task = [[NSTask alloc] init];
    [task setLaunchPath:proofdPath];
    [task setArguments:@[@"polish"]];
    [task setStandardInput:inputPipe];
    [task setStandardOutput:outputPipe];
    [task setStandardError:errorPipe];

    NSData *textData = [text dataUsingEncoding:NSUTF8StringEncoding];
    [[inputPipe fileHandleForWriting] writeData:textData];
    [[inputPipe fileHandleForWriting] closeFile];

    [task launch];
    [task waitUntilExit];

    if ([task terminationStatus] != 0) {
        NSData *errData = [[errorPipe fileHandleForReading] readDataToEndOfFile];
        NSString *errMsg = [[NSString alloc] initWithData:errData encoding:NSUTF8StringEncoding];
        if (!errMsg || errMsg.length == 0) errMsg = @"unknown error";
        logMsg(@"proofd failed (exit %ld): %@", (long)[task terminationStatus], errMsg);
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

- (NSString *)findProofd {
    NSString *home = getenv("HOME") ? @(getenv("HOME")) : @"";
    NSArray *candidates = @[
        @"/opt/homebrew/bin/proofd",
        @"/usr/local/bin/proofd",
        [home stringByAppendingPathComponent:@".cargo/bin/proofd"],
        [home stringByAppendingPathComponent:@".local/bin/proofd"],
        [home stringByAppendingPathComponent:@".bin/proofd"],
    ];
    for (NSString *path in candidates) {
        if ([[NSFileManager defaultManager] isExecutableFileAtPath:path]) {
            logMsg(@"Found proofd at: %@", path);
            return path;
        }
    }

    // Fall back to PATH
    NSTask *whichTask = [[NSTask alloc] init];
    [whichTask setLaunchPath:@"/usr/bin/which"];
    [whichTask setArguments:@[@"proofd"]];
    NSPipe *pipe = [NSPipe pipe];
    [whichTask setStandardOutput:pipe];
    [whichTask setStandardError:nil];
    [whichTask launch];
    [whichTask waitUntilExit];
    NSData *data = [[pipe fileHandleForReading] readDataToEndOfFile];
    NSString *path = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (path) {
        path = [path stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
        if (path.length > 0) {
            logMsg(@"Found proofd via PATH: %@", path);
            return path;
        }
    }
    logMsg(@"proofd not found anywhere");
    return nil;
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
