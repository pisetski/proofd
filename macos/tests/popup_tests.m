#import <AppKit/AppKit.h>
#import "../ProofreadProcess.h"
#import "../ResultPanel.h"
#import "../ProofreadService.h"

static void check(BOOL condition, NSString *message) {
    if (!condition) { fprintf(stderr, "FAIL: %s\n", message.UTF8String); exit(1); }
}

static void pump(NSTimeInterval seconds) {
    NSDate *end = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (end.timeIntervalSinceNow > 0) {
        [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    }
}

static void awaitReady(ResultPanel *panel) {
    NSDate *end = [NSDate dateWithTimeIntervalSinceNow:5];
    while ([[panel valueForKey:@"busy"] boolValue] && end.timeIntervalSinceNow > 0) pump(0.01);
    check(![[panel valueForKey:@"busy"] boolValue], @"request completes without blocking the main run loop");
}

static NSString *status(ResultPanel *panel) {
    return [[panel valueForKey:@"statusLabel"] stringValue];
}

static NSString *clipboardText(NSPasteboard *pb) {
    return [pb stringForType:NSPasteboardTypeString];
}

static void put(NSPasteboard *pb, NSString *text) {
    [pb clearContents];
    check([pb setString:text forType:NSPasteboardTypeString], @"test pasteboard write");
}

static void snapshot(ResultPanel *panel, NSString *name) {
    const char *directory = getenv("PROOFD_TEST_SNAPSHOT_DIR");
    if (!directory) return;
    NSView *view = panel.window.contentView;
    NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
    [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
    NSString *file = [@(directory) stringByAppendingPathComponent:[name stringByAppendingPathExtension:@"png"]];
    [[bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:file atomically:YES];
}

static void clickWindow(NSWindow *window) {
    NSEvent *event = [NSEvent mouseEventWithType:NSEventTypeLeftMouseDown
        location:NSMakePoint(8, 8) modifierFlags:0 timestamp:0
        windowNumber:window.windowNumber context:nil eventNumber:0 clickCount:1 pressure:1];
    [NSApp sendEvent:event];
}

int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        NSString *dir = [NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
        NSFileManager *fm = NSFileManager.defaultManager;
        check([fm createDirectoryAtPath:dir withIntermediateDirectories:YES attributes:nil error:NULL], @"fixture directory");
        NSString *executable = [dir stringByAppendingPathComponent:@"proofd-stub"];
        NSString *script = @"#!/bin/sh\n"
            "if [ \"$1\" = '--service-settings' ]; then\n"
            "  if [ -f \"$0.fail-settings\" ]; then exit 1; fi\n"
            "  if [ -f \"$0.slow-settings\" ]; then sleep 0.3; fi\n"
            "  echo '{\"show_result_popup\":true}'; exit\n"
            "fi\n"
            "input=$(cat)\n"
            "case \"$input\" in\n"
            " slow) sleep 0.4; printf 'Old result';;\n"
            " fail) printf 'provider unavailable' >&2; exit 1;;\n"
            " empty) exit 0;;\n"
            " unchanged) printf 'unchanged';;\n"
            " long) yes 'A sentence that fills multiple lines.' | head -n 400;;\n"
            " *) sleep 0.1; printf 'Corrected text.';;\n"
            "esac\n";
        check([script writeToFile:executable atomically:YES encoding:NSUTF8StringEncoding error:NULL], @"fixture script");
        check([fm setAttributes:@{NSFilePosixPermissions: @0700} ofItemAtPath:executable error:NULL], @"fixture executable");

        // More than a pipe buffer on stdin, stdout and stderr, including Unicode.
        NSString *large = [@"🌳é\n" stringByPaddingToLength:200000 withString:@"🌳é\n" startingAtIndex:0];
        ProofreadProcessResult *process = ProofreadRunProcess(@"/bin/sh", @[@"-c", @"cat; head -c 200000 /dev/zero >&2"], large);
        check(process.error == nil && [process.output isEqualToString:large], @"large Unicode streams do not deadlock or truncate");
        check(process.standardError.length == 200000, @"stderr drains concurrently");
        process = ProofreadRunProcess(@"/bin/sh", @[@"-c", @"exit 1"], large);
        check(process.error != nil, @"a child closing stdin early does not kill the service with SIGPIPE");
        process = ProofreadRunProcess(@"/missing/proofd", @[], @"input");
        check(process.error != nil, @"launch failures are returned, not thrown");
        process = ProofreadRunProcess(executable, @[], @"fail");
        check(process.error != nil && [process.standardError containsString:@"provider unavailable"], @"nonzero exit preserves diagnostics");

        // A unique pasteboard avoids touching the user's clipboard during tests.
        NSPasteboard *clipboard = [NSPasteboard pasteboardWithUniqueName];
        ResultPanel *panel = [[ResultPanel alloc] initWithPasteboard:clipboard];
        put(clipboard, @"Before");
        [panel beginProofreading:@"input" executable:executable];
        check([status(panel) containsString:@"Proofreading"], @"loading feedback appears immediately");
        snapshot(panel, @"loading");
        check(!panel.window.keyWindow && !NSApp.active, @"showing panel does not activate app or take keyboard focus");
        awaitReady(panel);
        check([clipboardText(clipboard) isEqualToString:@"Corrected text."], @"success automatically copies plain text");
        check([status(panel) containsString:@"Copied"], @"success confirms copy");
        check(panel.window.visible, @"result remains available for review");
        check([[[panel valueForKey:@"resultView"] string] isEqualToString:@"Corrected text."], @"result is displayed");
        snapshot(panel, @"result");

        [panel beginProofreading:@"long" executable:executable];
        awaitReady(panel);
        NSTextView *resultView = [panel valueForKey:@"resultView"];
        NSScrollView *scrollView = resultView.enclosingScrollView;
        [resultView.layoutManager ensureLayoutForTextContainer:resultView.textContainer];
        [resultView scrollRangeToVisible:NSMakeRange(resultView.string.length, 0)];
        check(scrollView.contentView.bounds.origin.y > 0, @"long results can scroll to the end");
        snapshot(panel, @"long-result");
        check(NSWidth(resultView.frame) <= NSWidth(scrollView.contentView.bounds) + 1,
              [NSString stringWithFormat:@"result text wraps within visible width: document=%g viewport=%g", NSWidth(resultView.frame), NSWidth(scrollView.contentView.bounds)]);
        check(NSHeight(panel.window.frame) < 420, @"long result panel stays compact");
        snapshot(panel, @"long-result");

        [panel beginProofreading:@"input" executable:executable];
        put(clipboard, @"Newer clipboard");
        awaitReady(panel);
        check([clipboardText(clipboard) isEqualToString:@"Newer clipboard"], @"new clipboard content survives completion");
        check([status(panel) containsString:@"Ready"], @"clipboard conflict offers manual copy");
        [[panel valueForKey:@"actionButton"] performClick:nil];
        check([clipboardText(clipboard) isEqualToString:@"Corrected text."], @"Copy explicitly replaces clipboard");
        check([status(panel) containsString:@"Copied"], @"manual copy confirms success");

        put(clipboard, @"Keep");
        [panel beginProofreading:@"unchanged" executable:executable];
        awaitReady(panel);
        check([status(panel) containsString:@"No changes needed"], @"unchanged input gets clear feedback");
        check([clipboardText(clipboard) isEqualToString:@"Keep"], @"unchanged input leaves clipboard alone");

        [panel beginProofreading:@"slow" executable:executable];
        [panel beginProofreading:@"input" executable:executable];
        awaitReady(panel);
        pump(0.5);
        check([clipboardText(clipboard) isEqualToString:@"Corrected text."], @"superseded result cannot overwrite newer result");

        put(clipboard, @"Keep after close");
        [panel beginProofreading:@"slow" executable:executable];
        [panel close];
        pump(0.6);
        check(!panel.window.visible, @"closed panel is not reopened by completion");
        check([clipboardText(clipboard) isEqualToString:@"Keep after close"], @"closed request cannot copy later");

        // Deliver real local AppKit events; no global key monitoring or input
        // injection into another application is needed for this regression.
        NSPanel *otherWindow = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 100, 100)
            styleMask:NSWindowStyleMaskNonactivatingPanel backing:NSBackingStoreBuffered defer:NO];
        [panel beginProofreading:@"slow" executable:executable];
        clickWindow(otherWindow);
        check(!panel.window.visible, @"clicking outside dismisses a pending popup");
        pump(0.6);
        check([clipboardText(clipboard) isEqualToString:@"Keep after close"], @"outside click discards pending clipboard writes");
        [panel beginProofreading:@"input" executable:executable];
        awaitReady(panel);
        clickWindow(panel.window);
        check(panel.window.visible, @"clicks inside the popup keep the result available");
        clickWindow(otherWindow);
        check(!panel.window.visible, @"outside click dismisses a completed popup");
        check([clipboardText(clipboard) isEqualToString:@"Corrected text."], @"dismissal keeps the copied result available to paste");
        check([panel valueForKey:@"globalMouseMonitor"] == nil && [panel valueForKey:@"localMouseMonitor"] == nil,
              @"closing removes both mouse monitors");
        [otherWindow close];
        put(clipboard, @"Keep after close");

        for (NSString *input in @[@"fail", @"empty"]) {
            [panel beginProofreading:input executable:executable];
            awaitReady(panel);
            NSButton *button = [panel valueForKey:@"actionButton"];
            check([button.title isEqualToString:@"Retry"] && button.enabled, @"failed or empty output offers retry");
            check([clipboardText(clipboard) isEqualToString:@"Keep after close"], @"failure never copies diagnostics or empty text");
            snapshot(panel, @"error");
        }
        // Change the provider, then Retry must use the saved selection.
        NSString *recovered = @"#!/bin/sh\ncat\n";
        [recovered writeToFile:executable atomically:NO encoding:NSUTF8StringEncoding error:NULL];
        [[panel valueForKey:@"actionButton"] performClick:nil];
        awaitReady(panel);
        check([status(panel) containsString:@"No changes needed"], @"Retry resubmits saved input");
        [panel close];

        [script writeToFile:executable atomically:NO encoding:NSUTF8StringEncoding error:NULL];
        ProofreadService *service = [[ProofreadService alloc] initWithExecutable:executable clipboard:clipboard];
        NSPasteboard *selection = [NSPasteboard pasteboardWithUniqueName];
        put(selection, @"input");
        NSString *error = nil;
        [service proofreadService:selection userData:nil error:&error];
        check(error == nil, @"popup service starts successfully");
        check([selection stringForType:NSPasteboardTypeString] == nil, @"popup returns no replacement text (including original or empty text)");
        ResultPanel *servicePanel = [service valueForKey:@"resultPanel"];
        awaitReady(servicePanel);
        check([clipboardText(clipboard) isEqualToString:@"Corrected text."], @"popup service delivers to clipboard");
        [servicePanel close];

        // A new invocation revokes old work even when configuration now fails.
        put(selection, @"slow");
        [service proofreadService:selection userData:nil error:&error];
        NSString *failSettings = [executable stringByAppendingString:@".fail-settings"];
        [@"" writeToFile:failSettings atomically:YES encoding:NSUTF8StringEncoding error:NULL];
        put(clipboard, @"Keep after bad settings");
        put(selection, @"input");
        [service proofreadService:selection userData:nil error:&error];
        check(error != nil, @"settings failure is reported");
        pump(0.6);
        check([clipboardText(clipboard) isEqualToString:@"Keep after bad settings"] && !servicePanel.window.visible,
              @"failed new invocation still retires the old panel and completion");
        [fm removeItemAtPath:failSettings error:NULL];
        error = nil;

        // waitUntilExit pumps the main run loop: old completions can fire while
        // the next settings query is still running.
        put(selection, @"slow");
        [service proofreadService:selection userData:nil error:&error];
        pump(0.2);
        NSString *slowSettings = [executable stringByAppendingString:@".slow-settings"];
        [@"" writeToFile:slowSettings atomically:YES encoding:NSUTF8StringEncoding error:NULL];
        put(clipboard, @"Keep through settings");
        put(selection, @"input");
        [service proofreadService:selection userData:nil error:&error];
        check([clipboardText(clipboard) isEqualToString:@"Keep through settings"],
              @"old completion cannot copy during new settings lookup");
        awaitReady(servicePanel);
        [servicePanel close];

        put(selection, @"input");
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 50 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            put(clipboard, @"Copied during settings");
        });
        [service proofreadService:selection userData:nil error:&error];
        awaitReady(servicePanel);
        check([clipboardText(clipboard) isEqualToString:@"Copied during settings"],
              @"clipboard protection starts at service invocation, before settings lookup");
        [servicePanel close];
        [fm removeItemAtPath:slowSettings error:NULL];

        NSString *replacementScript = [script stringByReplacingOccurrencesOfString:@"popup\":true" withString:@"popup\":false"];
        [replacementScript writeToFile:executable atomically:NO encoding:NSUTF8StringEncoding error:NULL];
        put(selection, @"input");
        put(clipboard, @"Do not change in replacement mode");
        [service proofreadService:selection userData:nil error:&error];
        check(error == nil && [clipboardText(selection) isEqualToString:@"Corrected text."], @"default flow returns replacement to Services");
        check([clipboardText(clipboard) isEqualToString:@"Do not change in replacement mode"], @"default flow does not use global clipboard");

        [@"#!/bin/sh\necho bad-config >&2\nexit 1\n" writeToFile:executable atomically:NO encoding:NSUTF8StringEncoding error:NULL];
        put(selection, @"original");
        [service proofreadService:selection userData:nil error:&error];
        check(error != nil && [clipboardText(selection) isEqualToString:@"original"], @"invalid settings fail without replacing text");

        // Empty return data must not delete a native text selection.
        NSTextView *editor = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 200, 100)];
        editor.string = @"Original selection";
        editor.selectedRange = NSMakeRange(0, editor.string.length);
        [selection clearContents];
        [editor readSelectionFromPasteboard:selection];
        check([editor.string isEqualToString:@"Original selection"] && editor.selectedRange.length == editor.string.length,
              @"no return types preserves native text and selection");

        [selection releaseGlobally];
        [clipboard releaseGlobally];
        [fm removeItemAtPath:dir error:NULL];
        puts("ok: native process, popup, clipboard, lifecycle, retry and service tests");
    }
    return 0;
}
