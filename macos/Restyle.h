#import <AppKit/AppKit.h>

// `revised` styled like `original`: unchanged text keeps its own attributes, and a fix takes
// the attributes of the text it replaced, so a corrected word stays bold or stays a link.
NSAttributedString *revisedTextStyledLike(NSAttributedString *original, NSString *revised);
