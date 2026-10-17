#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

@interface ProofreadService : NSObject
- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error;
@end

@implementation ProofreadService

- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    NSString *text = [pasteboard stringForType:NSPasteboardTypeString];
    if (!text || text.length == 0) {
        if (error) *error = @"No text to proofread";
        return;
    }

    NSString *proofdPath = [self findProofd];
    if (!proofdPath) {
        if (error) *error = @"proofd binary not found";
        return;
    }

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
        if (error) {
            *error = [NSString stringWithFormat:@"proofd failed (exit %ld): %@", (long)[task terminationStatus], errMsg];
        }
        return;
    }

    NSData *outputData = [[outputPipe fileHandleForReading] readDataToEndOfFile];
    NSString *result = [[NSString alloc] initWithData:outputData encoding:NSUTF8StringEncoding];
    if (!result) {
        if (error) *error = @"Failed to read proofd output";
        return;
    }

    [pasteboard clearContents];
    [pasteboard setString:result forType:NSPasteboardTypeString];
}

- (NSString *)findProofd {
    NSArray *candidates = @[
        @"/opt/homebrew/bin/proofd",
        @"/usr/local/bin/proofd",
        [NSString stringWithFormat:@"%s/.cargo/bin/proofd", getenv("HOME")],
        [NSString stringWithFormat:@"%s/.local/bin/proofd", getenv("HOME")],
        [NSString stringWithFormat:@"%s/bin/proofd", getenv("HOME")],
    ];
    for (NSString *path in candidates) {
        if ([[NSFileManager defaultManager] isExecutableFileAtPath:path]) {
            return path;
        }
    }

    // Fall back to PATH
    NSTask *whichTask = [[NSTask alloc] init];
    [whichTask setLaunchPath:@"/usr/bin/which"];
    [whichTask setArguments:@[@"proofd"]];
    NSPipe *pipe = [NSPipe pipe];
    [whichTask setStandardOutput:pipe];
    [whichTask launch];
    [whichTask waitUntilExit];
    NSData *data = [[pipe fileHandleForReading] readDataToEndOfFile];
    NSString *path = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    if (path) {
        path = [path stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
        if (path.length > 0) return path;
    }
    return nil;
}

@end

@interface AppDelegate : NSObject <NSApplicationDelegate>
@end

@implementation AppDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    NSRegisterServicesProvider([[ProofreadService alloc] init], @"proofd");
    // Terminate after 10 seconds if no service request arrives
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(10 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        [NSApp terminate:nil];
    });
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
