// Run by tests/restyle.rs. Prints each failed expectation and exits 1 if any failed.
#import "../Restyle.h"

static int failures = 0;

static void expect(BOOL ok, NSString *what) {
    if (ok) return;
    failures++;
    fprintf(stderr, "FAIL %s\n", what.UTF8String);
}

static NSFont *regularFont(void) {
    return [NSFont systemFontOfSize:12];
}

static NSFont *boldFont(void) {
    return [NSFont boldSystemFontOfSize:12];
}

static NSDictionary *plainStyle(void) {
    return @{NSFontAttributeName: regularFont()};
}

static NSDictionary *boldStyle(void) {
    return @{NSFontAttributeName: boldFont()};
}

// Joins (text, attributes) pairs into one attributed string.
static NSAttributedString *styledText(NSArray *pieces) {
    NSMutableAttributedString *text = [[NSMutableAttributedString alloc] init];
    for (NSUInteger i = 0; i < pieces.count; i += 2) {
        [text appendAttributedString:[[NSAttributedString alloc] initWithString:pieces[i] attributes:pieces[i + 1]]];
    }
    return text;
}

static void expectStyled(NSAttributedString *actual, NSAttributedString *expected, NSString *name) {
    BOOL same = [actual isEqualToAttributedString:expected];
    expect(same, [NSString stringWithFormat:@"%@\n  got:      %@\n  expected: %@", name, actual, expected]);
}

int main(void) {
    @autoreleasepool {
        NSAttributedString *unchanged = styledText(@[@"keep ", plainStyle(), @"this", boldStyle(), @" as is", plainStyle()]);
        expectStyled(revisedTextStyledLike(unchanged, unchanged.string), unchanged, @"unchanged text keeps every run");

        expectStyled(revisedTextStyledLike(styledText(@[@"I ", plainStyle(), @"beleive", boldStyle(), @" it", plainStyle()]),
                                           @"I believe it"),
                     styledText(@[@"I ", plainStyle(), @"believe", boldStyle(), @" it", plainStyle()]),
                     @"a fixed word keeps the style of the word it replaces");

        NSURL *docs = [NSURL URLWithString:@"https://example.com"];
        NSDictionary *link = @{NSFontAttributeName: regularFont(), NSLinkAttributeName: docs};
        expectStyled(revisedTextStyledLike(styledText(@[@"see ", plainStyle(), @"the docs", link, @" for detials", plainStyle()]),
                                           @"See the docs for details."),
                     styledText(@[@"See ", plainStyle(), @"the docs", link, @" for details.", plainStyle()]),
                     @"a link keeps its exact range when nearby words change");

        expectStyled(revisedTextStyledLike(styledText(@[@"tomorow", boldStyle(), @" if the test pass", plainStyle()]),
                                           @"tomorrow, if the tests pass"),
                     styledText(@[@"tomorrow,", boldStyle(), @" if the tests pass", plainStyle()]),
                     @"inserted punctuation takes the style of the text before it");

        expectStyled(revisedTextStyledLike(styledText(@[@"its", boldStyle(), @" fine", plainStyle()]), @"It's fine"),
                     styledText(@[@"It's", boldStyle(), @" fine", plainStyle()]),
                     @"a fix at the very start keeps the first word's style");

        NSAttributedString *original = styledText(@[[@"" stringByPaddingToLength:4000 withString:@"a " startingAtIndex:0], boldStyle()]);
        NSString *rewritten = [@"" stringByPaddingToLength:5000 withString:@"b " startingAtIndex:0];
        NSAttributedString *restyled = revisedTextStyledLike(original, rewritten);
        expect([restyled.string isEqualToString:rewritten], @"a full rewrite keeps the revised text");
        expect([[restyled attribute:NSFontAttributeName atIndex:4999 effectiveRange:nil] isEqual:boldFont()],
               @"a full rewrite still carries the original style");

        expectStyled(revisedTextStyledLike(styledText(@[@"x", boldStyle()]), @""), [[NSAttributedString alloc] initWithString:@""],
                     @"empty revised text is empty");

        if (failures == 0) printf("ok\n");
        return failures == 0 ? 0 : 1;
    }
}
