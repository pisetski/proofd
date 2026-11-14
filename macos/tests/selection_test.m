// Run by tests/objc_units.rs. Prints each failed expectation and exits 1 if any failed.
#import "../Selection.h"

static int failures = 0;

static void expect(BOOL ok, NSString *what) {
    if (ok) return;
    failures++;
    fprintf(stderr, "FAIL %s\n", what.UTF8String);
}

static NSDictionary *boldStyle(void) {
    return @{NSFontAttributeName: [NSFont boldSystemFontOfSize:12]};
}

static NSAttributedString *boldText(NSString *text) {
    return [[NSAttributedString alloc] initWithString:text attributes:boldStyle()];
}

static NSString *escaped(NSString *text) {
    return [[text stringByReplacingOccurrencesOfString:@"\t" withString:@"\\t"] stringByReplacingOccurrencesOfString:@"\n"
                                                                                                         withString:@"\\n"];
}

static void expectSelection(NSString *plain, NSAttributedString *rich, NSString *text, BOOL styled, NSString *name) {
    Selection *selection = [Selection selectionWithPlainCopy:plain richCopy:rich];
    expect([selection.text isEqualToString:text],
           [NSString stringWithFormat:@"%@: text \"%@\", expected \"%@\"", name, escaped(selection.text), escaped(text)]);
    expect((selection.styled != nil) == styled, [NSString stringWithFormat:@"%@: styled is %@", name, selection.styled]);
    if (styled) {
        expect([selection.styled.string isEqualToString:text], [NSString stringWithFormat:@"%@: styled text differs", name]);
        NSFont *font = [selection.styled attribute:NSFontAttributeName atIndex:0 effectiveRange:nil];
        expect([font isEqual:boldStyle()[NSFontAttributeName]], [NSString stringWithFormat:@"%@: lost the bold", name]);
    }
}

int main(void) {
    @autoreleasepool {
        expect([Selection selectionWithPlainCopy:nil richCopy:nil] == nil, @"no copies is no selection");
        expectSelection(@"Test test", nil, @"Test test", NO, @"a plain copy alone is proofread as is");
        expectSelection(nil, boldText(@"Test test"), @"Test test", YES, @"a rich copy alone is used for both");
        expectSelection(@"Test test", boldText(@"Test test"), @"Test test", YES, @"matching copies keep the styles");
        expectSelection(@"Test test", boldText(@"Test test\n"), @"Test test", YES,
                        @"a line break only the rich copy has stays out");
        expectSelection(@"Test test\n", boldText(@"Test test"), @"Test test\n", NO,
                        @"a rich copy missing the selection's line break isn't used");

        expectSelection(@"* Test test", boldText(@"\t•\t\t•\tTest test\n"), @"Test test", YES,
                        @"Notes' list item leaves its bullet out of both copies");
        expectSelection(@"* One\n* Two\nThree", boldText(@"\t•\tOne\n\t•\tTwo\nThree\n"), @"One\nTwo\nThree", YES,
                        @"each list item loses its bullet, and other lines stay");
        expectSelection(@"1. One", boldText(@"\t1.\tOne"), @"One", YES, @"a numbered item loses its number");
        expectSelection(@"\tfoo\tbar", boldText(@"\tfoo\tbar"), @"\tfoo\tbar", YES,
                        @"tabs in a line both copies agree on are text, not a bullet");
        expectSelection(@"* Test test", boldText(@"Test test"), @"* Test test", NO,
                        @"an asterisk the rich copy doesn't mark as a bullet is text");
        expectSelection(@"One\nTwo", boldText(@"One"), @"One\nTwo", NO, @"copies with different lines fall back to plain text");

        Selection *list = [Selection selectionWithPlainCopy:@"* One\n* Two" richCopy:boldText(@"\t•\tOne\n\t•\tTwo")];
        expect(list.bulletsLeftOut == 2, [NSString stringWithFormat:@"counted %lu bullets, expected 2", (unsigned long)list.bulletsLeftOut]);

        if (failures == 0) printf("ok\n");
        return failures == 0 ? 0 : 1;
    }
}
