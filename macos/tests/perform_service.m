// Invokes "Proofread Selection" exactly like the Services menu does and prints the result.
// NSPerformService returns YES even when AppKit drops the request, so the caller must
// check that the text actually changed. Exit codes: 0 changed, 1 unchanged, 2 not run.
#import <AppKit/AppKit.h>

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2) {
            fprintf(stderr, "usage: %s <text>\n", argv[0]);
            return 2;
        }
        NSString *input = @(argv[1]);
        NSPasteboard *pb = [NSPasteboard pasteboardWithUniqueName];
        [pb clearContents];
        [pb setString:input forType:NSPasteboardTypeString];

        if (!NSPerformService(@"Proofread Selection", pb)) {
            fprintf(stderr, "NSPerformService failed: service not registered or not enabled\n");
            return 2;
        }

        NSString *output = [pb stringForType:NSPasteboardTypeString];
        printf("%s\n", output.UTF8String ?: "");
        [pb releaseGlobally];
        return (output.length > 0 && ![output isEqualToString:input]) ? 0 : 1;
    }
}
