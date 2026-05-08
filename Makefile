.PHONY: build clean srpm rpm install

NAME     := explore-ai-agent
VERSION  := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
ARCHIVE  := $(NAME)-$(VERSION).tar.gz
BUILDDIR := /tmp/$(NAME)-build

## Build release binary
build:
	cargo build --release

## Clean build artifacts
clean:
	cargo clean
	rm -rf $(BUILDDIR) $(ARCHIVE)

## Create source tarball for RPM
dist: clean
	mkdir -p $(BUILDDIR)/$(NAME)-$(VERSION)
	cp -r \
		Cargo.toml Cargo.lock build.rs Makefile \
		src/ csrc/ tests/ config.template.yaml \
		$(BUILDDIR)/$(NAME)-$(VERSION)/
	tar -C $(BUILDDIR) -czf $(ARCHIVE) $(NAME)-$(VERSION)
	rm -rf $(BUILDDIR)

## Build RPM using mock (recommended for clean chroot build)
srpm: dist
	rpmbuild -ts $(ARCHIVE)

## Build RPM locally with rpmbuild
rpm: dist
	rpmbuild -tb $(ARCHIVE)

## Install locally (requires cargo build --release first)
install: build
	install -D -m 755 target/release/$(NAME) $(DESTDIR)/usr/bin/$(NAME)
	install -D -m 644 config.template.yaml $(DESTDIR)/etc/$(NAME)/config.yaml
	install -d -m 755 $(DESTDIR)/var/lib/$(NAME)/workspace
