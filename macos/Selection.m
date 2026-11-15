#import "Selection.h"

static const NSUInteger kMaxBulletLength = 16;
static const NSUInteger kMaxListNumberLength = 3;
static const NSUInteger kMaxRichMarkerLength = 6;

@implementation Selection

- (instancetype)initWithText:(NSString *)text
                  styledText:(NSAttributedString *)styledText
              bulletsLeftOut:(NSUInteger)bullets {
    if (!(self = [super init])) return nil;
    _text = text;
    _styledText = styledText;
    _bulletsLeftOut = bullets;
    return self;
}

- (BOOL)containsAttachment {
    NSString *attachmentCharacter = [NSString stringWithFormat:@"%C", (unichar)NSAttachmentCharacter];
    return [_text containsString:attachmentCharacter];
}

static BOOL hasOneStyle(NSAttributedString *text) {
    if (text.length == 0) return YES;
    NSRange run;
    [text attributesAtIndex:0 longestEffectiveRange:&run inRange:NSMakeRange(0, text.length)];
    return run.length == text.length;
}

// AppKit's RTF names the system font Helvetica Neue, so only a plain result keeps the app's own font.
static NSAttributedString *stylesWorthKeeping(NSAttributedString *styled, NSString *text) {
    return [styled.string isEqualToString:text] && !hasOneStyle(styled) ? styled : nil;
}

static NSUInteger notesTabbedListMarkersLength(NSString *richLine) {
    NSUInteger end = 0;
    while (end + 2 < richLine.length && [richLine characterAtIndex:end] == '\t') {
        NSUInteger searchLength = MIN(kMaxRichMarkerLength + 1, richLine.length - end - 1);
        NSRange closingTab = [richLine rangeOfString:@"\t" options:NSLiteralSearch range:NSMakeRange(end + 1, searchLength)];
        if (closingTab.location == NSNotFound || closingTab.location == end + 1) break;
        NSString *marker = [richLine substringWithRange:NSMakeRange(end + 1, closingTab.location - end - 1)];
        if ([marker rangeOfCharacterFromSet:[NSCharacterSet whitespaceCharacterSet]].location != NSNotFound) break;
        end = NSMaxRange(closingTab);
    }
    return end;
}

static BOOL isSymbolBullet(NSString *marker) {
    return marker.length == 1 && ![[NSCharacterSet alphanumericCharacterSet] characterIsMember:[marker characterAtIndex:0]];
}

static BOOL isEnumeratorBullet(NSString *marker) {
    unichar closing = [marker characterAtIndex:marker.length - 1];
    NSString *enumerator = [marker substringToIndex:marker.length - 1];
    BOOL onlyAlphanumerics =
        [enumerator rangeOfCharacterFromSet:[NSCharacterSet alphanumericCharacterSet].invertedSet].location == NSNotFound;
    return (closing == '.' || closing == ')') && enumerator.length <= kMaxListNumberLength && onlyAlphanumerics;
}

static BOOL isPlainBullet(NSString *prefix, BOOL itemHasContent) {
    NSCharacterSet *whitespace = [NSCharacterSet whitespaceCharacterSet];
    NSString *marker = [prefix stringByTrimmingCharactersInSet:whitespace];
    if (marker.length == 0 || prefix.length > kMaxBulletLength) return NO;
    BOOL spacedFromContent = [whitespace characterIsMember:[prefix characterAtIndex:prefix.length - 1]];
    if (itemHasContent && !spacedFromContent) return NO;
    return marker.length == 1 ? isSymbolBullet(marker) : isEnumeratorBullet(marker);
}

static NSUInteger listMarkerLengthOfBulletedLine(NSString *plainLine, NSString *richLine) {
    NSUInteger markerLength = notesTabbedListMarkersLength(richLine);
    if (markerLength == 0 || [plainLine isEqualToString:richLine]) return 0;
    NSString *content = [richLine substringFromIndex:markerLength];
    if (content.length > 0 && ![plainLine hasSuffix:content]) return 0;
    NSString *plainBullet = [plainLine substringToIndex:plainLine.length - content.length];
    return isPlainBullet(plainBullet, content.length > 0) ? markerLength : 0;
}

static NSMutableAttributedString *withoutTrailingNewlineOnlyRichHas(NSAttributedString *rich, NSString *plain) {
    NSMutableAttributedString *trimmed = [rich mutableCopy];
    if ([trimmed.string hasSuffix:@"\n"] && ![plain hasSuffix:@"\n"]) {
        [trimmed deleteCharactersInRange:NSMakeRange(trimmed.length - 1, 1)];
    }
    return trimmed;
}

+ (instancetype)selectionWithPlainCopy:(NSString *)plain richCopy:(NSAttributedString *)rich {
    if (!plain && !rich) return nil;
    if (!plain) return [[self alloc] initWithText:rich.string styledText:stylesWorthKeeping(rich, rich.string) bulletsLeftOut:0];
    if (!rich) return [[self alloc] initWithText:plain styledText:nil bulletsLeftOut:0];

    NSMutableAttributedString *styled = withoutTrailingNewlineOnlyRichHas(rich, plain);
    NSArray<NSString *> *plainLines = [plain componentsSeparatedByString:@"\n"];
    NSArray<NSString *> *richLines = [styled.string componentsSeparatedByString:@"\n"];
    if (plainLines.count != richLines.count) return [[self alloc] initWithText:plain styledText:nil bulletsLeftOut:0];

    // Notes' paragraphs keep their own bullets, so a bullet it also wrote as text would show twice.
    NSMutableArray<NSString *> *textLines = [NSMutableArray array];
    NSUInteger bullets = 0, richLineStart = 0;
    for (NSUInteger i = 0; i < plainLines.count; i++) {
        NSString *richLine = richLines[i];
        NSUInteger markerLength = listMarkerLengthOfBulletedLine(plainLines[i], richLine);
        if (markerLength > 0) {
            [styled deleteCharactersInRange:NSMakeRange(richLineStart, markerLength)];
            bullets++;
        }
        NSString *line = markerLength > 0 ? [richLine substringFromIndex:markerLength] : plainLines[i];
        [textLines addObject:line];
        richLineStart += richLine.length - markerLength + 1;
    }
    NSString *text = [textLines componentsJoinedByString:@"\n"];
    return [[self alloc] initWithText:text styledText:stylesWorthKeeping(styled, text) bulletsLeftOut:bullets];
}

@end
