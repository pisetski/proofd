DOMAIN := gui/$(shell id -u)
BIN := proofd
SRC := target/release/$(BIN)
DEST := $(HOME)/.cargo/bin/$(BIN)
LABEL := com.proofd
PLIST := $(HOME)/Library/LaunchAgents/$(LABEL).plist
LOG_OUT := /tmp/proofd.out.log
LOG_ERR := /tmp/proofd.err.log
SERVICE_LOG := $(HOME)/Library/Logs/proofd-service.log
SERVICE_LOG_PREDICATE := process == "proofd-service-host" AND (messageType == error OR messageType == fault OR (sender == "AppKit" AND NOT category IN {"Appearance", "AutomaticTermination", "StateRestoration"}))

.NOTPARALLEL:
.PHONY: build install unload load restart reinstall logs clear-logs test-service

build:
	cargo build --release

install: build
	mkdir -p $(HOME)/.cargo/bin
	cp $(SRC) $(DEST)

unload:
	launchctl bootout $(DOMAIN)/$(LABEL) 2>/dev/null || true

load:
	launchctl bootstrap $(DOMAIN) $(PLIST) || (sleep 1 && launchctl bootstrap $(DOMAIN) $(PLIST))

clear-logs:
	: > $(LOG_OUT)
	: > $(LOG_ERR)
	: > $(SERVICE_LOG)

restart: unload clear-logs load

reinstall: install restart

# Daemon logs, Services host log, and AppKit's own messages for the Services
# host (e.g. "Cannot find service provider"), which only reach the system log.
logs:
	@touch $(LOG_OUT) $(LOG_ERR) $(SERVICE_LOG)
	@/usr/bin/log stream --style compact --predicate '$(SERVICE_LOG_PREDICATE)' & \
	LOG_PID=$$!; trap 'kill $$LOG_PID 2>/dev/null' EXIT; \
	tail -F $(LOG_ERR) $(LOG_OUT) $(SERVICE_LOG)

# End-to-end check of the installed Services menu item; needs a GUI session and a working provider.
test-service:
	bash macos/test-service.sh
