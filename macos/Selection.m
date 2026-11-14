#import "Selection.h"

static const NSUInteger kMaxBulletLength = 16;
static const NSUInteger kMaxListNumberLength = 3;
static const NSUInteger kMaxRichMarkerLength = 6;

@implementation Selection

- (instancetype)initWithText:(NSString *)text styled:(NSAttributedString *)styled bulletsLeftOut:(NSUInteger)bullets {
    if (!(self = [super init])) return nil;
    _text = text;
    _styled = styled;
    _bulletsLeftOut = bullets;
    return self;
}

// Length of the "\t•\t" markers Notes' rich copy starts a list item with, often twice over.
static NSUInteger richListMarkerLength(NSString *line) {
    NSUInteger end = 0;
    while (end + 2 < line.length && [line characterAtIndex:end] == '\t') {
        NSUInteger searchLength = MIN(kMaxRichMarkerLength + 1, line.length - end - 1);
        NSRange close = [line rangeOfString:@"\t" options:NSLiteralSearch range:NSMakeRange(end + 1, searchLength)];
        if (close.location == NSNotFound || close.location == end + 1) break;
        NSString *marker = [line substringWithRange:NSMakeRange(end + 1, close.location - end - 1)];
        if ([marker rangeOfCharacterFromSet:[NSCharacterSet whitespaceCharacterSet]].location != NSNotFound) break;
        end = NSMaxRange(close);
    }
    return end;
}

// "* ", "- ", "• ", "1. " or "a) ", possibly indented; the space is absent before an empty item.
static BOOL isBullet(NSString *prefix, BOOL beforeContent) {
    NSCharacterSet *whitespace = [NSCharacterSet whitespaceCharacterSet];
    NSString *marker = [prefix stringByTrimmingCharactersInSet:whitespace];
    if (marker.length == 0 || prefix.length > kMaxBulletLength) return NO;
    if (beforeContent && ![whitespace characterIsMember:[prefix characterAtIndex:prefix.length - 1]]) return NO;
    NSCharacterSet *alphanumerics = [NSCharacterSet alphanumericCharacterSet];
    if (marker.length == 1) return ![alphanumerics characterIsMember:[marker characterAtIndex:0]];
    unichar closing = [marker characterAtIndex:marker.length - 1];
    NSString *number = [marker substringToIndex:marker.length - 1];
    return (closing == '.' || closing == ')') && number.length <= kMaxListNumberLength &&
           [number rangeOfCharacterFromSet:alphanumerics.invertedSet].location == NSNotFound;
}

static BOOL hasOneStyle(NSAttributedString *text) {
    if (text.length == 0) return YES;
    NSRange run;
    [text attributesAtIndex:0 longestEffectiveRange:&run inRange:NSMakeRange(0, text.length)];
    return run.length == text.length;
}

// Notes writes a list item's bullet as text in both copies, "* item" in the plain one. Put
// back, it would show twice, since the paragraph keeps its own bullet; so it's left out
// wherever the rich copy confirms the line is a list item.
+ (instancetype)selectionWithPlainCopy:(NSString *)plain richCopy:(NSAttributedString *)rich {
    if (!plain) return rich ? [[self alloc] initWithText:rich.string styled:hasOneStyle(rich) ? nil : rich bulletsLeftOut:0] : nil;
    if (!rich) return [[self alloc] initWithText:plain styled:nil bulletsLeftOut:0];

    NSMutableAttributedString *styled = [rich mutableCopy];
    if ([styled.string hasSuffix:@"\n"] && ![plain hasSuffix:@"\n"]) {
        [styled deleteCharactersInRange:NSMakeRange(styled.length - 1, 1)];
    }
    NSArray<NSString *> *plainLines = [plain componentsSeparatedByString:@"\n"];
    NSArray<NSString *> *richLines = [styled.string componentsSeparatedByString:@"\n"];
    if (plainLines.count != richLines.count) return [[self alloc] initWithText:plain styled:nil bulletsLeftOut:0];

    NSMutableArray<NSString *> *textLines = [NSMutableArray array];
    NSUInteger bullets = 0, richLineStart = 0;
    for (NSUInteger i = 0; i < plainLines.count; i++) {
        NSString *plainLine = plainLines[i], *richLine = richLines[i];
        NSUInteger markerLength = richListMarkerLength(richLine);
        NSString *content = [richLine substringFromIndex:markerLength];
        BOOL endsWithContent = content.length == 0 || [plainLine hasSuffix:content];
        BOOL listItem = markerLength > 0 && ![plainLine isEqualToString:richLine] && endsWithContent &&
                        isBullet([plainLine substringToIndex:plainLine.length - content.length], content.length > 0);
        if (listItem) {
            [styled deleteCharactersInRange:NSMakeRange(richLineStart, markerLength)];
            bullets++;
        }
        [textLines addObject:listItem ? content : plainLine];
        richLineStart += (listItem ? content.length : richLine.length) + 1;
    }
    NSString *text = [textLines componentsJoinedByString:@"\n"];
    BOOL keepStyles = [styled.string isEqualToString:text] && !hasOneStyle(styled);
    return [[self alloc] initWithText:text styled:keepStyles ? styled : nil bulletsLeftOut:bullets];
}

@end
