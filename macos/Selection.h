#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

// What to proofread, from the plain and rich copies an app puts on the services pasteboard.
@interface Selection : NSObject

// The plain copy, less any list bullets the app wrote as text.
@property (readonly) NSString *text;
// `text` with the app's styles, or nil when the rich copy doesn't match it or has one
// style throughout: AppKit's RTF names the system font Helvetica Neue, so only a plain
// result keeps the app's own font.
@property (readonly, nullable) NSAttributedString *styled;
@property (readonly) NSUInteger bulletsLeftOut;

+ (nullable instancetype)selectionWithPlainCopy:(nullable NSString *)plain richCopy:(nullable NSAttributedString *)rich;

@end

NS_ASSUME_NONNULL_END
