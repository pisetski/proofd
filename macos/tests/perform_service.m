#import <AppKit/AppKit.h>

typedef NS_ENUM(int, ServiceOutcome) {
    ServiceOutcomeChanged = 0,
    ServiceOutcomeUnchanged = 1,
    ServiceOutcomeNotRun = 2,
    ServiceOutcomeLostBold = 3,
    ServiceOutcomeGainedLineBreak = 4,
};

static NSFont *boldFont(void) {
    return [NSFont boldSystemFontOfSize:12];
}

static NSAttributedString *attributedFromRTF(NSData *rtf) {
    return rtf ? [[NSAttributedString alloc] initWithRTF:rtf documentAttributes:nil] : nil;
}

static BOOL startsBold(NSData *rtf) {
    NSAttributedString *text = attributedFromRTF(rtf);
    if (text.length == 0) return NO;
    NSFont *font = [text attribute:NSFontAttributeName atIndex:0 effectiveRange:nil];
    return (font.fontDescriptor.symbolicTraits & NSFontDescriptorTraitBold) != 0;
}

static NSData *notesLikeRTFWithBoldFirstWord(NSString *input) {
    NSString *withTrailingBreakOnlyRichHas = [input stringByAppendingString:@"\n"];
    NSMutableAttributedString *paragraph = [[NSMutableAttributedString alloc]
        initWithString:withTrailingBreakOnlyRichHas
            attributes:@{NSFontAttributeName: [NSFont systemFontOfSize:12]}];
    NSUInteger firstWordEnd = [input rangeOfString:@" "].location;
    if (firstWordEnd == NSNotFound) firstWordEnd = input.length;
    [paragraph addAttribute:NSFontAttributeName value:boldFont() range:NSMakeRange(0, firstWordEnd)];
    return [paragraph RTFFromRange:NSMakeRange(0, paragraph.length) documentAttributes:@{}];
}

static ServiceOutcome outcomeOn(NSPasteboard *pasteboard, NSString *input, BOOL rich) {
    NSString *output = [pasteboard stringForType:NSPasteboardTypeString];
    printf("%s\n", output.UTF8String ?: "");
    NSData *rtf = [pasteboard dataForType:NSPasteboardTypeRTF];
    NSString *richOutput = attributedFromRTF(rtf).string ?: @"";
    if (output.length == 0 || [output isEqualToString:input]) return ServiceOutcomeUnchanged;
    if (rich && !startsBold(rtf)) return ServiceOutcomeLostBold;
    if ([output hasSuffix:@"\n"] || [richOutput hasSuffix:@"\n"]) return ServiceOutcomeGainedLineBreak;
    return ServiceOutcomeChanged;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        BOOL rich = argc == 3 && strcmp(argv[1], "--rich") == 0;
        if (argc != 2 && !rich) {
            fprintf(stderr, "usage: %s [--rich] <text>\n", argv[0]);
            return ServiceOutcomeNotRun;
        }
        NSString *input = @(argv[argc - 1]);
        NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
        [pasteboard clearContents];
        if (rich) {
            [pasteboard declareTypes:@[NSPasteboardTypeRTF, NSPasteboardTypeString] owner:nil];
            [pasteboard setData:notesLikeRTFWithBoldFirstWord(input) forType:NSPasteboardTypeRTF];
        }
        [pasteboard setString:input forType:NSPasteboardTypeString];

        if (!NSPerformService(@"Proofread Selection", pasteboard)) {
            fprintf(stderr, "NSPerformService failed: service not registered or not enabled\n");
            return ServiceOutcomeNotRun;
        }
        // NSPerformService also returns YES when AppKit drops the request, so only a changed text proves it ran.
        ServiceOutcome outcome = outcomeOn(pasteboard, input, rich);
        [pasteboard releaseGlobally];
        return outcome;
    }
}
