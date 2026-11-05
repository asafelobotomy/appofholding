# appOFholding: build and install.
#   make                      build release binaries
#   make install PREFIX=/usr  install (use DESTDIR=... for packaging)
#   make uninstall
PREFIX   ?= /usr/local
DESTDIR  ?=
PROFILE  ?= release
CARGOFLAGS ?= --locked

BIN      = $(PREFIX)/bin
SHARE    = $(PREFIX)/share
TARGET   = target/$(PROFILE)
CLI      = $(TARGET)/appofholding
GUI      = $(TARGET)/appofholding-gui
APPID    = io.github.appofholding

ifeq ($(PROFILE),release)
  PROFILEFLAG = --release
else
  PROFILEFLAG =
endif

.PHONY: all build install uninstall test clean

all: build

build:
	cargo build $(PROFILEFLAG) $(CARGOFLAGS) --workspace

test:
	cargo test $(CARGOFLAGS) --workspace

install: build
	install -Dm755 $(CLI) $(DESTDIR)$(BIN)/appofholding
	install -Dm755 $(GUI) $(DESTDIR)$(BIN)/appofholding-gui
	install -Dm644 packaging/$(APPID).desktop $(DESTDIR)$(SHARE)/applications/$(APPID).desktop
	install -Dm644 packaging/$(APPID).svg $(DESTDIR)$(SHARE)/icons/hicolor/scalable/apps/$(APPID).svg
	install -Dm644 packaging/$(APPID).metainfo.xml $(DESTDIR)$(SHARE)/metainfo/$(APPID).metainfo.xml
	install -Dm644 LICENSE $(DESTDIR)$(SHARE)/licenses/appofholding/LICENSE
	install -d $(DESTDIR)$(SHARE)/man/man1
	$(CLI) man $(DESTDIR)$(SHARE)/man/man1
	install -d $(DESTDIR)$(SHARE)/bash-completion/completions $(DESTDIR)$(SHARE)/zsh/site-functions $(DESTDIR)$(SHARE)/fish/vendor_completions.d
	$(CLI) completions bash > $(DESTDIR)$(SHARE)/bash-completion/completions/appofholding
	$(CLI) completions zsh  > $(DESTDIR)$(SHARE)/zsh/site-functions/_appofholding
	$(CLI) completions fish > $(DESTDIR)$(SHARE)/fish/vendor_completions.d/appofholding.fish
	install -Dm644 README.md $(DESTDIR)$(SHARE)/doc/appofholding/README.md
	install -d $(DESTDIR)$(SHARE)/doc/appofholding
	install -m644 docs/*.md $(DESTDIR)$(SHARE)/doc/appofholding/

uninstall:
	rm -f $(DESTDIR)$(BIN)/appofholding $(DESTDIR)$(BIN)/appofholding-gui
	rm -f $(DESTDIR)$(SHARE)/applications/$(APPID).desktop
	rm -f $(DESTDIR)$(SHARE)/icons/hicolor/scalable/apps/$(APPID).svg
	rm -f $(DESTDIR)$(SHARE)/metainfo/$(APPID).metainfo.xml
	rm -rf $(DESTDIR)$(SHARE)/licenses/appofholding $(DESTDIR)$(SHARE)/doc/appofholding
	rm -f $(DESTDIR)$(SHARE)/man/man1/appofholding*.1
	rm -f $(DESTDIR)$(SHARE)/bash-completion/completions/appofholding $(DESTDIR)$(SHARE)/zsh/site-functions/_appofholding $(DESTDIR)$(SHARE)/fish/vendor_completions.d/appofholding.fish

clean:
	cargo clean
