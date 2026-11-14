#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

// What to proofread, from the plain and rich copies an app puts on the services pasteboard.
@interface Selection : NSObject

// Its revision replaces the selection.
@property (readonly) NSString *text;
// `text` with the app's styles, or nil when the rich copy doesn't match it.
@property (readonly, nullable) NSAttributedString *styled;
@property (readonly) NSUInteger bulletsLeftOut;

+ (nullable instancetype)selectionWithPlainCopy:(nullable NSString *)plain richCopy:(nullable NSAttributedString *)rich;

@end

NS_ASSUME_NONNULL_END
