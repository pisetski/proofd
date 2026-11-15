#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

@interface Selection : NSObject

@property (readonly) NSString *text;
@property (readonly, nullable) NSAttributedString *styledText;
@property (readonly) NSUInteger bulletsLeftOut;
@property (readonly) BOOL containsAttachment;

+ (nullable instancetype)selectionWithPlainCopy:(nullable NSString *)plain richCopy:(nullable NSAttributedString *)rich;

@end

NS_ASSUME_NONNULL_END
