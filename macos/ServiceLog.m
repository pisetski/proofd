#import "ServiceLog.h"

static const unsigned long long kMaxLogBytes = 1024 * 1024;

static NSString *logDirectory(void) {
    return [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Logs"];
}

static NSString *logPath(void) {
    return [logDirectory() stringByAppendingPathComponent:@"proofd-service.log"];
}

static void keepOneOldLogIfFull(NSString *path) {
    NSFileManager *files = [NSFileManager defaultManager];
    if ([[files attributesOfItemAtPath:path error:nil] fileSize] < kMaxLogBytes) return;
    NSString *oldPath = [path stringByAppendingString:@".old"];
    [files removeItemAtPath:oldPath error:nil];
    [files moveItemAtPath:path toPath:oldPath error:nil];
}

static void appendToLog(NSString *path, NSData *data) {
    NSFileHandle *log = [NSFileHandle fileHandleForWritingAtPath:path];
    if (!log) {
        [[NSFileManager defaultManager] createDirectoryAtPath:logDirectory()
                                  withIntermediateDirectories:YES
                                                   attributes:nil
                                                        error:nil];
        [data writeToFile:path atomically:YES];
        return;
    }
    [log seekToEndOfFile];
    [log writeData:data];
    [log closeFile];
}

void serviceLog(NSString *format, ...) {
    va_list args;
    va_start(args, format);
    NSString *message = [[NSString alloc] initWithFormat:format arguments:args];
    va_end(args);
    NSString *path = logPath();
    keepOneOldLogIfFull(path);
    NSString *line = [NSString stringWithFormat:@"[%@] %@\n", [[NSDate date] description], message];
    appendToLog(path, [line dataUsingEncoding:NSUTF8StringEncoding]);
}
