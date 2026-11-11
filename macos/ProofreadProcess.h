#import <Foundation/Foundation.h>

@interface ProofreadProcessResult : NSObject
@property(nonatomic, copy) NSString *output;
@property(nonatomic, copy) NSString *standardError;
@property(nonatomic, copy) NSString *error;
@end

// Blocking: use a background queue for interactive work. All pipes drain
// concurrently, including stdin, so large selections/output cannot deadlock.
ProofreadProcessResult *ProofreadRunProcess(NSString *executable, NSArray<NSString *> *arguments, NSString *input);
