DOMAIN := gui/$(shell id -u)
BIN := proofd
SRC := target/release/$(BIN)
DEST := $(HOME)/.cargo/bin/$(BIN)
LABEL := com.proofd
PLIST := $(HOME)/Library/LaunchAgents/$(LABEL).plist

.NOTPARALLEL:
.PHONY: build install unload load restart reinstall logs

build:
	cargo build --release

install: build
	mkdir -p $(HOME)/.cargo/bin
	cp $(SRC) $(DEST)

unload:
	launchctl bootout $(DOMAIN)/$(LABEL) 2>/dev/null || true

load:
	launchctl bootstrap $(DOMAIN) $(PLIST) || (sleep 1 && launchctl bootstrap $(DOMAIN) $(PLIST))

restart: unload load

reinstall: install restart

logs:
	tail -f /tmp/proofd.out.log
