// Invokes "Proofread Selection" exactly like the Services menu does and prints the result.
// NSPerformService returns YES even when AppKit drops the request, so the caller must
// check that the text actually changed. With --rich the text goes in bold, as RTF, and
// must come back bold. Exit codes: 0 changed, 1 unchanged, 2 not run, 3 lost its bold.
#import <AppKit/AppKit.h>

static NSFont *boldFont(void) {
    return [NSFont boldSystemFontOfSize:12];
}

static BOOL isBoldThroughout(NSData *rtf) {
    NSAttributedString *text = rtf ? [[NSAttributedString alloc] initWithRTF:rtf documentAttributes:nil] : nil;
    if (text.length == 0) return NO;
    __block BOOL bold = YES;
    [text enumerateAttribute:NSFontAttributeName inRange:NSMakeRange(0, text.length) options:0
                  usingBlock:^(NSFont *font, NSRange range, BOOL *stop) {
        if (!(font.fontDescriptor.symbolicTraits & NSFontDescriptorTraitBold)) bold = NO;
    }];
    return bold;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        BOOL rich = argc == 3 && strcmp(argv[1], "--rich") == 0;
        if (argc != 2 && !rich) {
            fprintf(stderr, "usage: %s [--rich] <text>\n", argv[0]);
            return 2;
        }
        NSString *input = @(argv[argc - 1]);
        NSPasteboard *pb = [NSPasteboard pasteboardWithUniqueName];
        [pb clearContents];
        if (rich) {
            NSAttributedString *bold = [[NSAttributedString alloc] initWithString:input
                                                                       attributes:@{NSFontAttributeName: boldFont()}];
            [pb writeObjects:@[bold]];
        } else {
            [pb setString:input forType:NSPasteboardTypeString];
        }

        if (!NSPerformService(@"Proofread Selection", pb)) {
            fprintf(stderr, "NSPerformService failed: service not registered or not enabled\n");
            return 2;
        }

        NSString *output = [pb stringForType:NSPasteboardTypeString];
        printf("%s\n", output.UTF8String ?: "");
        BOOL lostBold = rich && !isBoldThroughout([pb dataForType:NSPasteboardTypeRTF]);
        [pb releaseGlobally];
        if (output.length == 0 || [output isEqualToString:input]) return 1;
        return lostBold ? 3 : 0;
    }
}
