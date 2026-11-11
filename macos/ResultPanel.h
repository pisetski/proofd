#import <AppKit/AppKit.h>

@interface ResultPanel : NSWindowController <NSWindowDelegate>
- (instancetype)initWithPasteboard:(NSPasteboard *)pasteboard;
- (void)beginProofreading:(NSString *)input executable:(NSString *)executable;
- (void)beginProofreading:(NSString *)input executable:(NSString *)executable clipboardChangeCount:(NSInteger)changeCount;
@end
