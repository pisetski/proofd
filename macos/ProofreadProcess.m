#import "ProofreadProcess.h"
#import <fcntl.h>

@implementation ProofreadProcessResult
@end

ProofreadProcessResult *ProofreadRunProcess(NSString *executable, NSArray<NSString *> *arguments, NSString *input) {
    ProofreadProcessResult *result = [[ProofreadProcessResult alloc] init];
    result.output = @"";
    result.standardError = @"";
    if (!executable.length) {
        result.error = @"proofd binary missing from proofd.app; reinstall it";
        return result;
    }

    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:executable];
    task.arguments = arguments;
    NSPipe *inputPipe = [NSPipe pipe];
    NSPipe *outputPipe = [NSPipe pipe];
    NSPipe *errorPipe = [NSPipe pipe];
    // Suppress SIGPIPE on this descriptor only. A child that rejects input may
    // close stdin before our writer finishes; report EPIPE instead of dying.
    if (fcntl(inputPipe.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1) == -1) {
        result.error = @"Failed to configure proofd input pipe";
        return result;
    }
    task.standardInput = inputPipe;
    task.standardOutput = outputPipe;
    task.standardError = errorPipe;
    NSError *launchError = nil;
    if (![task launchAndReturnError:&launchError]) {
        result.error = [NSString stringWithFormat:@"Could not start proofd: %@", launchError.localizedDescription];
        return result;
    }

    dispatch_group_t group = dispatch_group_create();
    dispatch_queue_t queue = dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0);
    __block NSData *outputData;
    __block NSData *errorData;
    __block NSError *outputError;
    __block NSError *stderrError;
    __block NSError *inputError;
    dispatch_group_async(group, queue, ^{
        outputData = [outputPipe.fileHandleForReading readDataToEndOfFileAndReturnError:&outputError];
    });
    dispatch_group_async(group, queue, ^{
        errorData = [errorPipe.fileHandleForReading readDataToEndOfFileAndReturnError:&stderrError];
    });
    dispatch_group_async(group, queue, ^{
        // A child may reject input and close its pipe early. Its exit status
        // and stderr describe that failure; a failed write must not crash us.
        [inputPipe.fileHandleForWriting writeData:[input dataUsingEncoding:NSUTF8StringEncoding] error:&inputError];
        [inputPipe.fileHandleForWriting closeAndReturnError:NULL];
    });
    [task waitUntilExit];
    dispatch_group_wait(group, DISPATCH_TIME_FOREVER);
    [outputPipe.fileHandleForReading closeAndReturnError:NULL];
    [errorPipe.fileHandleForReading closeAndReturnError:NULL];

    result.standardError = [[NSString alloc] initWithData:errorData ?: [NSData data] encoding:NSUTF8StringEncoding] ?: @"Unreadable diagnostics";
    if (task.terminationStatus != 0) {
        result.error = [NSString stringWithFormat:@"proofd failed (exit %d): %@", task.terminationStatus, result.standardError];
    } else if (outputError || stderrError) {
        result.error = @"Failed to read proofd output";
    } else if (inputError) {
        result.error = @"Failed to send selected text to proofd";
    } else {
        NSString *text = [[NSString alloc] initWithData:outputData ?: [NSData data] encoding:NSUTF8StringEncoding];
        if (text) result.output = text;
        else result.error = @"proofd returned invalid UTF-8";
    }
    return result;
}
