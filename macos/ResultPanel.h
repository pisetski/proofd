#import <AppKit/AppKit.h>

@interface ResultPanel : NSWindowController <NSWindowDelegate>
- (instancetype)initWithPasteboard:(NSPasteboard *)pasteboard;
- (void)showMessage:(NSString *)message;
- (void)beginProofreading:(NSString *)input executable:(NSString *)executable;
- (void)beginProofreading:(NSString *)input executable:(NSString *)executable clipboardChangeCount:(NSInteger)changeCount;
- (void)beginProofreading:(NSString *)input executable:(NSString *)executable clipboardChangeCount:(NSInteger)changeCount showPopup:(BOOL)showPopup;
@end
