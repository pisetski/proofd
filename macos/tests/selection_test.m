#import "../Selection.h"

static int failures = 0;

static void expect(BOOL ok, NSString *what) {
    if (ok) return;
    failures++;
    fprintf(stderr, "FAIL %s\n", what.UTF8String);
}

static NSFont *boldFont(void) {
    return [NSFont boldSystemFontOfSize:12];
}

static NSAttributedString *uniformlyBold(NSString *text) {
    return [[NSAttributedString alloc] initWithString:text attributes:@{NSFontAttributeName: boldFont()}];
}

static NSAttributedString *boldExceptLastCharacter(NSString *text) {
    NSMutableAttributedString *styled = [uniformlyBold(text) mutableCopy];
    NSString *trimmed = [text stringByTrimmingCharactersInSet:[NSCharacterSet newlineCharacterSet]];
    [styled addAttribute:NSFontAttributeName value:[NSFont systemFontOfSize:12] range:NSMakeRange(trimmed.length - 1, 1)];
    return styled;
}

static NSString *escaped(NSString *text) {
    return [[text stringByReplacingOccurrencesOfString:@"\t" withString:@"\\t"] stringByReplacingOccurrencesOfString:@"\n"
                                                                                                         withString:@"\\n"];
}

static void expectSelection(NSString *plain, NSAttributedString *rich, NSString *text, BOOL keepsStyles, NSString *name) {
    Selection *selection = [Selection selectionWithPlainCopy:plain richCopy:rich];
    expect([selection.text isEqualToString:text],
           [NSString stringWithFormat:@"%@: text \"%@\", expected \"%@\"", name, escaped(selection.text), escaped(text)]);
    expect((selection.styledText != nil) == keepsStyles, [NSString stringWithFormat:@"%@: styled is %@", name, selection.styledText]);
    if (keepsStyles) {
        expect([selection.styledText.string isEqualToString:text], [NSString stringWithFormat:@"%@: styled text differs", name]);
        NSFont *font = [selection.styledText attribute:NSFontAttributeName atIndex:0 effectiveRange:nil];
        expect([font isEqual:boldFont()], [NSString stringWithFormat:@"%@: lost the bold", name]);
    }
}

static NSPasteboard *serviceCopyOfBoldTextWithAnInlineImage(void) {
    NSTextView *view = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 300, 100)];
    view.richText = YES;
    view.importsGraphics = YES;
    NSWindow *window = [[NSWindow alloc] initWithContentRect:view.frame
                                                   styleMask:NSWindowStyleMaskTitled
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    window.contentView = view;
    NSTextAttachment *image = [[NSTextAttachment alloc] init];
    image.image = [NSImage imageWithSystemSymbolName:@"star" accessibilityDescription:nil];
    NSMutableAttributedString *content = [uniformlyBold(@"Look ") mutableCopy];
    [content appendAttributedString:[NSAttributedString attributedStringWithAttachment:image]];
    [content appendAttributedString:[[NSAttributedString alloc] initWithString:@" here"]];
    [view.textStorage setAttributedString:content];
    [view selectAll:nil];

    NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
    [view writeSelectionToPasteboard:pasteboard types:@[@"NSRTFPboardType", @"NSStringPboardType"]];
    return pasteboard;
}

static Selection *selectionOn(NSPasteboard *pasteboard) {
    NSData *rtf = [pasteboard dataForType:NSPasteboardTypeRTF];
    NSAttributedString *rich = rtf ? [[NSAttributedString alloc] initWithRTF:rtf documentAttributes:nil] : nil;
    return [Selection selectionWithPlainCopy:[pasteboard stringForType:NSPasteboardTypeString] richCopy:rich];
}

int main(void) {
    @autoreleasepool {
        expect([Selection selectionWithPlainCopy:nil richCopy:nil] == nil, @"no copies is no selection");
        expectSelection(@"Test test", nil, @"Test test", NO, @"a plain copy alone is proofread as is");
        expectSelection(nil, boldExceptLastCharacter(@"Test test"), @"Test test", YES, @"a rich copy alone is used for both");
        expectSelection(@"Test test", boldExceptLastCharacter(@"Test test"), @"Test test", YES, @"matching copies keep the styles");
        expectSelection(@"Test test", uniformlyBold(@"Test test"), @"Test test", NO,
                        @"one style throughout goes back as plain text, keeping the app's font");
        expectSelection(nil, uniformlyBold(@"Test test"), @"Test test", NO, @"a rich-only copy in one style goes back plain");
        expectSelection(@"Test test", boldExceptLastCharacter(@"Test test\n"), @"Test test", YES,
                        @"a line break only the rich copy has stays out");
        expectSelection(@"Test test\n", boldExceptLastCharacter(@"Test test"), @"Test test\n", NO,
                        @"a rich copy missing the selection's line break isn't used");

        expectSelection(@"* Test test", boldExceptLastCharacter(@"\t•\t\t•\tTest test\n"), @"Test test", YES,
                        @"Notes' list item leaves its bullet out of both copies");
        expectSelection(@"* One\n* Two\nThree", boldExceptLastCharacter(@"\t•\tOne\n\t•\tTwo\nThree\n"), @"One\nTwo\nThree", YES,
                        @"each list item loses its bullet, and other lines stay");
        expectSelection(@"1. One", boldExceptLastCharacter(@"\t1.\tOne"), @"One", YES, @"a numbered item loses its number");
        expectSelection(@"* One\n* \n* Two", boldExceptLastCharacter(@"\t•\tOne\n\t•\t\n\t•\tTwo\n"), @"One\n\nTwo", YES,
                        @"an empty list item loses its bullet too");
        expectSelection(@"* One\n*\n* Two", boldExceptLastCharacter(@"\t•\tOne\n\t•\t\n\t•\tTwo\n"), @"One\n\nTwo", YES,
                        @"an empty list item's bullet may lack its space");

        expectSelection(@"\tfoo\tbar", boldExceptLastCharacter(@"\tfoo\tbar"), @"\tfoo\tbar", YES,
                        @"tabs in a line both copies agree on are text, not a bullet");
        expectSelection(@"Name\tAge", boldExceptLastCharacter(@"\tName\tAge"), @"Name\tAge", NO, @"a word before a tab is not a bullet");
        expectSelection(@"Total: 5", boldExceptLastCharacter(@"\tTotal:\t5"), @"Total: 5", NO, @"a label before a space is not a bullet");
        expectSelection(@"* Test test", boldExceptLastCharacter(@"Test test"), @"* Test test", NO,
                        @"an asterisk the rich copy doesn't mark as a bullet is text");
        expectSelection(@"One\nTwo", boldExceptLastCharacter(@"One"), @"One\nTwo", NO, @"copies with different lines fall back to plain text");

        Selection *list = [Selection selectionWithPlainCopy:@"* One\n* Two" richCopy:boldExceptLastCharacter(@"\t•\tOne\n\t•\tTwo")];
        expect(list.bulletsLeftOut == 2, [NSString stringWithFormat:@"counted %lu bullets, expected 2", (unsigned long)list.bulletsLeftOut]);

        Selection *withImage = selectionOn(serviceCopyOfBoldTextWithAnInlineImage());
        expect(withImage != nil, @"the text view put its selection on the pasteboard");
        expect(withImage.containsAttachment, @"a selection with an inline image is flagged");
        expect(![Selection selectionWithPlainCopy:@"Test test" richCopy:nil].containsAttachment, @"plain text has no attachment");
        expect(![Selection selectionWithPlainCopy:@"Test test" richCopy:boldExceptLastCharacter(@"Test test")].containsAttachment,
               @"styled text has no attachment");

        if (failures == 0) printf("ok\n");
        return failures == 0 ? 0 : 1;
    }
}
