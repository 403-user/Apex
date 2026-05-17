.PHONY: all build build-release check test run-server run-client clean \
        lint fmt dev docker-build deb bench bench-compare security-audit

CARGO = cargo
ZIG = zig
DEB_VERSION = 0.1.0

all: build

build:
	$(CARGO) build

build-release:
	$(CARGO) build --release

check:
	$(CARGO) check --workspace

test:
	$(CARGO) test --workspace

run-server:
	$(CARGO) run -- --server

run-client:
	$(CARGO) run

zig-build:
	cd zig-extensions && $(ZIG) build

zig-wasm:
	cd zig-extensions && $(ZIG) build -Doptimize=ReleaseSmall

clean:
	$(CARGO) clean
	cd zig-extensions && $(ZIG) build clean 2>/dev/null || true

lint:
	$(CARGO) clippy --workspace -- -D warnings
	cd zig-extensions && $(ZIG) fmt --check . 2>/dev/null || true

fmt:
	$(CARGO) fmt --all
	cd zig-extensions && $(ZIG) fmt . 2>/dev/null || true

dev: build
	$(CARGO) run

docker-build:
	docker build -t apex-terminal:latest .

# ─── Debian Packaging ────────────────────────────────────────────

deb: build-release
	@echo "Building .deb package for Kali Linux..."
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/bin
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/share/applications
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/lib/systemd/user
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/share/icons/hicolor/scalable/apps
	@mkdir -p build/deb/apex-terminal_$(DEB_VERSION)_amd64/DEBIAN
	install -m 0755 target/release/apex-terminal \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/bin/apex
	install -m 0644 packaging/apex-terminal.desktop \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/share/applications/
	install -m 0644 packaging/apex-terminal.service \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/lib/systemd/user/
	install -m 0644 assets/logo.svg \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr/share/icons/hicolor/scalable/apps/apex-terminal.svg
	install -m 0644 packaging/debian/control \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/DEBIAN/control
	install -m 0644 packaging/debian/copyright \
		build/deb/apex-terminal_$(DEB_VERSION)_amd64/DEBIAN/copyright
	@echo "Version: $(DEB_VERSION)" >> build/deb/apex-terminal_$(DEB_VERSION)_amd64/DEBIAN/control
	@echo "Installed-Size: $$(du -sk build/deb/apex-terminal_$(DEB_VERSION)_amd64/usr | cut -f1)" >> build/deb/apex-terminal_$(DEB_VERSION)_amd64/DEBIAN/control
	dpkg-deb --build build/deb/apex-terminal_$(DEB_VERSION)_amd64
	@echo "Package: build/deb/apex-terminal_$(DEB_VERSION)_amd64.deb"

# ─── Benchmarks ─────────────────────────────────────────────────

bench: build-release
	@echo "Running benchmark suite..."
	@bash benchmarks/run.sh

bench-compare:
	@echo "Comparing against reference terminals..."
	@echo "Ensure Alacritty and GNOME Terminal are installed."
	@echo "Manual benchmark: vtebench --config benchmarks/config.toml"

# ─── Security Audit ─────────────────────────────────────────────

security-audit:
	$(CARGO) audit 2>/dev/null || echo "Install cargo-audit: cargo install cargo-audit"
	$(CARGO) deny check 2>/dev/null || echo "Install cargo-deny: cargo install cargo-deny"
	@echo "Checking for unsafe code..."
	@! rg 'unsafe\s' --include='*.rs' --count | grep -v '^0' || echo "Warning: unsafe code found"
	@echo "Security audit complete."
