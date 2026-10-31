PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

APPLICATION_ID := io.github.kaii_lb.Overskride
RELEASE_BINARY := target/release/overskride
INSTALL_ROOT := $(DESTDIR)$(PREFIX)
SHARE_DIRECTORY := $(INSTALL_ROOT)/share
SCHEMA_DIRECTORY := $(SHARE_DIRECTORY)/glib-2.0/schemas
ICON_DIRECTORY := $(SHARE_DIRECTORY)/icons/hicolor
SYSTEMD_USER_DIRECTORY := $(INSTALL_ROOT)/lib/systemd/user
BINARY_DIRECTORY ?= $(PREFIX)/bin
CHECK_ROOT := $(CURDIR)/target/check-install

.PHONY: build install install-binary install-data install-desktop-entry install-metainfo install-schema install-icons install-systemd-unit uninstall check

build:
	$(CARGO) build --release --locked --package bindrune-gtk

install: install-binary install-data

install-binary:
	install -Dm755 $(RELEASE_BINARY) $(INSTALL_ROOT)/bin/overskride

install-data: install-desktop-entry install-metainfo install-schema install-icons install-systemd-unit
ifeq ($(DESTDIR),)
	glib-compile-schemas $(SCHEMA_DIRECTORY)
	-gtk-update-icon-cache -qtf $(ICON_DIRECTORY)
endif

install-desktop-entry:
	install -Dm644 data/$(APPLICATION_ID).desktop.in $(SHARE_DIRECTORY)/applications/$(APPLICATION_ID).desktop

install-metainfo:
	install -Dm644 data/$(APPLICATION_ID).appdata.xml.in $(SHARE_DIRECTORY)/metainfo/$(APPLICATION_ID).metainfo.xml

install-schema:
	install -Dm644 data/$(APPLICATION_ID).gschema.xml $(SCHEMA_DIRECTORY)/$(APPLICATION_ID).gschema.xml

install-icons:
	install -Dm644 data/icons/hicolor/scalable/apps/$(APPLICATION_ID).svg $(ICON_DIRECTORY)/scalable/apps/$(APPLICATION_ID).svg
	install -Dm644 data/icons/hicolor/symbolic/apps/$(APPLICATION_ID)-symbolic.svg $(ICON_DIRECTORY)/symbolic/apps/$(APPLICATION_ID)-symbolic.svg

install-systemd-unit:
	install -d $(SYSTEMD_USER_DIRECTORY)
	sed 's|@bindir@|$(BINARY_DIRECTORY)|' data/overskride.service.in > $(SYSTEMD_USER_DIRECTORY)/overskride.service
	chmod 644 $(SYSTEMD_USER_DIRECTORY)/overskride.service

uninstall:
	rm -f $(INSTALL_ROOT)/bin/overskride
	rm -f $(SHARE_DIRECTORY)/applications/$(APPLICATION_ID).desktop
	rm -f $(SHARE_DIRECTORY)/metainfo/$(APPLICATION_ID).metainfo.xml
	rm -f $(SCHEMA_DIRECTORY)/$(APPLICATION_ID).gschema.xml
	rm -f $(ICON_DIRECTORY)/scalable/apps/$(APPLICATION_ID).svg
	rm -f $(ICON_DIRECTORY)/symbolic/apps/$(APPLICATION_ID)-symbolic.svg
	rm -f $(SYSTEMD_USER_DIRECTORY)/overskride.service

check:
	$(CARGO) test --workspace --locked
	$(MAKE) install-data DESTDIR=$(CHECK_ROOT) PREFIX=
	desktop-file-validate $(CHECK_ROOT)/share/applications/$(APPLICATION_ID).desktop
	appstreamcli validate --no-net $(CHECK_ROOT)/share/metainfo/$(APPLICATION_ID).metainfo.xml
	glib-compile-schemas --strict --dry-run data
