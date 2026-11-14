// Invokes "Proofread Selection" exactly like the Services menu does and prints the result.
// NSPerformService returns YES even when AppKit drops the request, so the caller must
// check that the text actually changed. With --rich the text goes in bold, as RTF, and
// must come back bold. Like Notes, the RTF copy then ends in a line break the plain copy
// lacks, which must not reach the result.
// Exit codes: 0 changed, 1 unchanged, 2 not run, 3 lost its bold, 4 gained a line break.
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
            NSAttributedString *paragraph = [[NSAttributedString alloc] initWithString:[input stringByAppendingString:@"\n"]
                                                                            attributes:@{NSFontAttributeName: boldFont()}];
            [pb declareTypes:@[NSPasteboardTypeRTF, NSPasteboardTypeString] owner:nil];
            [pb setData:[paragraph RTFFromRange:NSMakeRange(0, paragraph.length) documentAttributes:@{}]
                forType:NSPasteboardTypeRTF];
        }
        [pb setString:input forType:NSPasteboardTypeString];

        if (!NSPerformService(@"Proofread Selection", pb)) {
            fprintf(stderr, "NSPerformService failed: service not registered or not enabled\n");
            return 2;
        }

        NSString *output = [pb stringForType:NSPasteboardTypeString];
        printf("%s\n", output.UTF8String ?: "");
        NSData *rtf = [pb dataForType:NSPasteboardTypeRTF];
        BOOL lostBold = rich && !isBoldThroughout(rtf);
        NSString *richOutput = rtf ? [[NSAttributedString alloc] initWithRTF:rtf documentAttributes:nil].string : @"";
        BOOL gainedBreak = [output hasSuffix:@"\n"] || [richOutput hasSuffix:@"\n"];
        [pb releaseGlobally];
        if (output.length == 0 || [output isEqualToString:input]) return 1;
        if (lostBold) return 3;
        return gainedBreak ? 4 : 0;
    }
}
