SERVICE_LOG := $(HOME)/Library/Logs/proofd-service.log
SERVICE_LOG_PREDICATE := process == "proofd-service-host" AND (messageType == error OR messageType == fault OR (sender == "AppKit" AND NOT category IN {"Appearance", "AutomaticTermination", "StateRestoration"}))

APP_PATH := $(HOME)/Applications/proofd.app
CLI_LINK := $(HOME)/.local/bin/proofd
LSREGISTER := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

.PHONY: build install uninstall logs clear-logs test-service

build:
	cargo build --release

install:
	bash contrib/install.sh

# Config and logs stay; remove ~/.config/proofd and $(SERVICE_LOG) by hand if wanted.
uninstall:
	pkill -f "$(APP_PATH)/Contents/MacOS/proofd-service-host" || true
	"$(LSREGISTER)" -u "$(APP_PATH)" || true
	rm -rf "$(APP_PATH)"
	[ ! -L "$(CLI_LINK)" ] || rm "$(CLI_LINK)"
	/System/Library/CoreServices/pbs -update

clear-logs:
	: > $(SERVICE_LOG)

# Services host log, plus AppKit's own messages for the host
# (e.g. "Cannot find service provider"), which only reach the system log.
logs:
	@touch $(SERVICE_LOG)
	@/usr/bin/log stream --style compact --predicate '$(SERVICE_LOG_PREDICATE)' & \
	LOG_PID=$$!; trap 'kill $$LOG_PID 2>/dev/null' EXIT; \
	tail -F $(SERVICE_LOG)

# End-to-end check of the installed Services menu item; needs a GUI session and a working provider.
test-service:
	bash macos/test-service.sh
