# Makefile for exynos-loke-rescue
# Deterministic Linux firmware recovery engine for Samsung Exynos SoCs

CARGO ?= cargo
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
RULESDIR ?= /etc/udev/rules.d
MANDIR ?= $(PREFIX)/share/man/man1
BASHCOMPDIR ?= /usr/share/bash-completion/completions
ZSHCOMPDIR ?= /usr/share/zsh/site-functions
FISHCOMPDIR ?= /usr/share/fish/vendor_completions.d

VERSION ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

.PHONY: all build release test check fmt lint clean install package-tarball audit docker-build help

all: build

help:
	@echo "Exynos LOKE Rescue Build Targets:"
	@echo "  build           Build debug binary"
	@echo "  release         Build optimized release binary with LTO & stripped symbols"
	@echo "  test            Run unit and integration tests"
	@echo "  check           Run cargo check"
	@echo "  fmt             Run cargo fmt"
	@echo "  lint            Run cargo clippy with strict warnings"
	@echo "  audit           Run release binary hardening and REA exposure audit"
	@echo "  package-tarball Assemble portable distribution archive (.tar.gz)"
	@echo "  install         Install binary and udev rules to system (requires root)"
	@echo "  docker-build    Build minimal multi-stage container image"
	@echo "  clean           Clean target/ and dist/ artifacts"

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test --all-targets

check:
	$(CARGO) check

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --all-targets -- -D warnings

audit: release
	./scripts/audit_release.sh target/release/exynos-loke-rescue

package-tarball: release
	./scripts/package_tarball.sh $(VERSION)

install: release
	install -D -m 0755 target/release/exynos-loke-rescue $(DESTDIR)$(BINDIR)/exynos-loke-rescue
	install -D -m 0644 udev/60-exynos-loke-rescue.rules $(DESTDIR)$(RULESDIR)/60-exynos-loke-rescue.rules
	@if [ -f target/man/exynos-loke-rescue.1 ]; then \
		install -D -m 0644 target/man/exynos-loke-rescue.1 $(DESTDIR)$(MANDIR)/exynos-loke-rescue.1; \
	fi
	@if [ -d $(DESTDIR)$(BASHCOMPDIR) ] && [ -f target/completions/exynos-loke-rescue.bash ]; then \
		install -D -m 0644 target/completions/exynos-loke-rescue.bash $(DESTDIR)$(BASHCOMPDIR)/exynos-loke-rescue; \
	fi
	@if [ -d $(DESTDIR)$(ZSHCOMPDIR) ] && [ -f target/completions/_exynos-loke-rescue ]; then \
		install -D -m 0644 target/completions/_exynos-loke-rescue $(DESTDIR)$(ZSHCOMPDIR)/_exynos-loke-rescue; \
	fi
	@if [ -d $(DESTDIR)$(FISHCOMPDIR) ] && [ -f target/completions/exynos-loke-rescue.fish ]; then \
		install -D -m 0644 target/completions/exynos-loke-rescue.fish $(DESTDIR)$(FISHCOMPDIR)/exynos-loke-rescue.fish; \
	fi
	@if command -v udevadm >/dev/null 2>&1; then \
		udevadm control --reload-rules || true; \
		udevadm trigger --subsystem-match=usb || true; \
	fi
	@echo "✓ Successfully installed exynos-loke-rescue to $(DESTDIR)$(BINDIR)"

docker-build:
	podman build -t ghcr.io/rebeeh/exynos-loke-rescue:latest -t ghcr.io/rebeeh/exynos-loke-rescue:v$(VERSION) .

clean:
	$(CARGO) clean
	rm -rf dist/ target/man/ target/completions/
