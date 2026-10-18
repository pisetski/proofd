SERVICE_LOG := $(HOME)/Library/Logs/proofd-service.log
SERVICE_LOG_PREDICATE := process == "proofd-service-host" AND (messageType == error OR messageType == fault OR (sender == "AppKit" AND NOT category IN {"Appearance", "AutomaticTermination", "StateRestoration"}))

.PHONY: build install logs clear-logs test-service

build:
	cargo build --release

install:
	bash contrib/install.sh

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
