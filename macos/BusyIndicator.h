#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

@interface CursorBusyIndicator : NSObject
- (void)finishWithSuccess;
- (void)finishWithFailure:(NSString *)reason;
@end

NS_ASSUME_NONNULL_END
