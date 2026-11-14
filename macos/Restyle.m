#import "Restyle.h"

typedef struct {
    NSUInteger start;
    NSUInteger length;
} Token;

typedef struct {
    const unichar *chars;
    const Token *tokens;
    NSInteger count;
} Tokens;

static const NSInteger kMaxEditsOfAProofread = 1000;
static const NSInteger kInsertedToken = -1;

static NSData *charactersOf(NSString *text) {
    NSMutableData *chars = [NSMutableData dataWithLength:text.length * sizeof(unichar)];
    [text getCharacters:chars.mutableBytes range:NSMakeRange(0, text.length)];
    return chars;
}

static NSData *wordsAndComposedCharacters(NSString *text, NSData *chars) {
    const unichar *units = chars.bytes;
    NSUInteger length = chars.length / sizeof(unichar);
    NSCharacterSet *wordCharacters = [NSCharacterSet alphanumericCharacterSet];
    NSMutableData *tokens = [NSMutableData data];
    for (NSUInteger start = 0, end; start < length; start = end) {
        if ([wordCharacters characterIsMember:units[start]]) {
            end = start + 1;
            while (end < length && [wordCharacters characterIsMember:units[end]]) end++;
        } else {
            end = NSMaxRange([text rangeOfComposedCharacterSequenceAtIndex:start]);
        }
        Token token = {start, end - start};
        [tokens appendBytes:&token length:sizeof token];
    }
    return tokens;
}

static Tokens tokensOf(NSData *chars, NSData *tokens) {
    return (Tokens){chars.bytes, tokens.bytes, (NSInteger)(tokens.length / sizeof(Token))};
}

static BOOL sameToken(Tokens a, NSInteger i, Tokens b, NSInteger j) {
    Token x = a.tokens[i], y = b.tokens[j];
    return x.length == y.length && memcmp(a.chars + x.start, b.chars + y.start, x.length * sizeof(unichar)) == 0;
}

// Step d of Myers' diff stores the furthest x on diagonals -d...d, so it starts at d².
static NSInteger furthestX(const NSInteger *history, NSInteger d, NSInteger k) {
    return history[d * d + d + k];
}

static BOOL alignByMyersDiff(Tokens before, Tokens after, NSInteger *keptFrom) {
    NSInteger n = before.count, m = after.count;
    NSInteger maxEdits = MIN(n + m, kMaxEditsOfAProofread);
    NSInteger offset = maxEdits + 1;
    NSMutableData *frontierData = [NSMutableData dataWithLength:(2 * offset + 1) * sizeof(NSInteger)];
    NSInteger *frontier = frontierData.mutableBytes;
    NSMutableData *historyData = [NSMutableData data];
    NSInteger edits = -1;
    for (NSInteger d = 0; d <= maxEdits && edits < 0; d++) {
        for (NSInteger k = -d; k <= d; k += 2) {
            BOOL down = k == -d || (k != d && frontier[offset + k - 1] < frontier[offset + k + 1]);
            NSInteger x = down ? frontier[offset + k + 1] : frontier[offset + k - 1] + 1;
            NSInteger y = x - k;
            while (x < n && y < m && sameToken(before, x, after, y)) {
                x++;
                y++;
            }
            frontier[offset + k] = x;
            if (x >= n && y >= m) edits = d;
        }
        [historyData appendBytes:frontier + offset - d length:(2 * d + 1) * sizeof(NSInteger)];
    }
    if (edits < 0) return NO;

    const NSInteger *history = historyData.bytes;
    for (NSInteger j = 0; j < m; j++) keptFrom[j] = kInsertedToken;
    NSInteger x = n, y = m;
    for (NSInteger d = edits; d > 0; d--) {
        NSInteger k = x - y;
        BOOL down = k == -d || (k != d && furthestX(history, d - 1, k - 1) < furthestX(history, d - 1, k + 1));
        NSInteger previousK = down ? k + 1 : k - 1;
        NSInteger previousX = furthestX(history, d - 1, previousK);
        NSInteger snakeStartX = down ? previousX : previousX + 1;
        while (x > snakeStartX) {
            x--;
            y--;
            keptFrom[y] = x;
        }
        x = previousX;
        y = previousX - previousK;
    }
    while (x > 0 && y > 0) {
        x--;
        y--;
        keptFrom[y] = x;
    }
    return YES;
}

static NSUInteger characterBeforeInsertionAt(NSUInteger location) {
    return location > 0 ? location - 1 : 0;
}

static void spreadEvenlyOver(NSUInteger *sourceOf, NSRange revised, NSRange original) {
    for (NSUInteger i = 0; i < revised.length; i++) {
        sourceOf[revised.location + i] = original.length > 0
            ? original.location + i * original.length / revised.length
            : characterBeforeInsertionAt(original.location);
    }
}

static void mapThroughAlignment(NSUInteger *sourceOf, Tokens before, Tokens after, const NSInteger *keptFrom,
                                NSUInteger originalLength, NSUInteger revisedLength) {
    NSUInteger originalEnd = 0, revisedEnd = 0;
    for (NSInteger j = 0; j <= after.count; j++) {
        if (j < after.count && keptFrom[j] == kInsertedToken) continue;
        Token revisedToken = j < after.count ? after.tokens[j] : (Token){revisedLength, 0};
        Token originalToken = j < after.count ? before.tokens[keptFrom[j]] : (Token){originalLength, 0};
        spreadEvenlyOver(sourceOf, NSMakeRange(revisedEnd, revisedToken.start - revisedEnd),
                         NSMakeRange(originalEnd, originalToken.start - originalEnd));
        for (NSUInteger i = 0; i < revisedToken.length; i++) {
            sourceOf[revisedToken.start + i] = originalToken.start + i;
        }
        revisedEnd = NSMaxRange(NSMakeRange(revisedToken.start, revisedToken.length));
        originalEnd = NSMaxRange(NSMakeRange(originalToken.start, originalToken.length));
    }
}

static NSAttributedString *withOriginalRuns(NSAttributedString *original, NSString *revised,
                                            const NSUInteger *sourceOf) {
    NSUInteger revisedLength = revised.length;
    NSMutableAttributedString *styled = [[NSMutableAttributedString alloc] initWithString:revised];
    for (NSUInteger start = 0, end; start < revisedLength; start = end) {
        NSRange run;
        NSDictionary *attributes = [original attributesAtIndex:sourceOf[start] effectiveRange:&run];
        end = start + 1;
        while (end < revisedLength && NSLocationInRange(sourceOf[end], run)) end++;
        end = NSMaxRange([revised rangeOfComposedCharacterSequencesForRange:NSMakeRange(start, end - start)]);
        [styled setAttributes:attributes range:NSMakeRange(start, end - start)];
    }
    return styled;
}

NSAttributedString *revisedTextStyledLike(NSAttributedString *original, NSString *revised) {
    NSUInteger originalLength = original.length, revisedLength = revised.length;
    if (originalLength == 0 || revisedLength == 0) return [[NSAttributedString alloc] initWithString:revised];

    NSData *originalChars = charactersOf(original.string), *revisedChars = charactersOf(revised);
    NSData *originalTokens = wordsAndComposedCharacters(original.string, originalChars);
    NSData *revisedTokens = wordsAndComposedCharacters(revised, revisedChars);
    Tokens before = tokensOf(originalChars, originalTokens), after = tokensOf(revisedChars, revisedTokens);

    NSMutableData *sourceData = [NSMutableData dataWithLength:revisedLength * sizeof(NSUInteger)];
    NSUInteger *sourceOf = sourceData.mutableBytes;
    NSMutableData *keptFrom = [NSMutableData dataWithLength:after.count * sizeof(NSInteger)];
    if (alignByMyersDiff(before, after, keptFrom.mutableBytes)) {
        mapThroughAlignment(sourceOf, before, after, keptFrom.bytes, originalLength, revisedLength);
    } else {
        spreadEvenlyOver(sourceOf, NSMakeRange(0, revisedLength), NSMakeRange(0, originalLength));
    }
    return withOriginalRuns(original, revised, sourceOf);
}
