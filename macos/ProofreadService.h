#import <AppKit/AppKit.h>

@interface ProofreadService : NSObject
- (instancetype)initWithExecutable:(NSString *)executable clipboard:(NSPasteboard *)clipboard;
- (void)proofreadService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error;
- (void)proofreadClipboardService:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error;
@end
