DOMAIN := gui/$(shell id -u)
BIN := proofd
SRC := target/release/$(BIN)
DEST := $(HOME)/.cargo/bin/$(BIN)
LABEL := com.proofd
PLIST := $(HOME)/Library/LaunchAgents/$(LABEL).plist
LOG_OUT := /tmp/proofd.out.log
LOG_ERR := /tmp/proofd.err.log

.NOTPARALLEL:
.PHONY: build install unload load restart reinstall logs clear-logs

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

restart: unload clear-logs load

reinstall: install restart

logs:
	tail -f $(LOG_ERR) $(LOG_OUT)
