# Installation

Bindrune talks to BlueZ, so the Bluetooth service has to run:

```bash
sudo systemctl enable --now bluetooth
```

File transfers also need `obexd` (often packaged as `bluez-obex`). Battery
levels need BlueZ's experimental features (`Experimental = true` in
`/etc/bluetooth/main.conf`).

## Try it from a checkout

On any distribution with Nix and flakes enabled:

```bash
git clone https://codeberg.org/wvhulle/bindrune
cd bindrune
nix develop
cargo run -p bindrune-gtk
```

Or run the packaged binary without cloning:

```bash
nix run git+https://codeberg.org/wvhulle/bindrune
```

No installation is needed to run from a checkout: the build compiles the
GSettings schema and the app uses it when no installed schema is found.

## NixOS and home-manager

Add the flake as an input. Its `overlays.default` adds `pkgs.bindrune`. Both
modules install the package and run `bindrune --background` as a systemd user
service, so pairing requests and incoming files are handled while the window is
closed.

```nix
{
  inputs.bindrune = {
    url = "git+https://codeberg.org/wvhulle/bindrune";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

- **NixOS:** import `bindrune.nixosModules.default` and set
  `services.bindrune.enable = true;`. The service starts with every graphical
  session.
- **home-manager:** import `bindrune.homeManagerModules.default` and set
  `services.bindrune.enable = true;`.

## Other distributions

Build and install with cargo and make. This needs Rust (preferably through
[rustup](https://rustup.rs), which picks up `rust-toolchain.toml`), the
development packages for GTK 4 (≥ 4.16), libadwaita (≥ 1.6), D-Bus, udev and
PulseAudio (libpulse), and `glib-compile-schemas`.

```bash
make
sudo make install            # PREFIX defaults to /usr/local
sudo make uninstall
```

`make install DESTDIR=… PREFIX=/usr` stages the files for distribution
packages. To start Bindrune hidden at login, enable the installed user
service:

```bash
systemctl --user enable --now bindrune
```

## Flatpak

```bash
flatpak-builder --user --install --force-clean flatpak-build page.codeberg.wvhulle.Bindrune.json
```

## Running in the background

Closing the window keeps Bindrune running. A tray icon (on desktops with
StatusNotifierItem support, such as KDE, or GNOME with the AppIndicator
extension) reopens the window. Pairing requests and incoming files show a
notification while the window is hidden or unfocused. Quit from the main menu,
the tray icon or with Ctrl+Q. Launching Bindrune from the app menu opens the
window of the instance that already runs.
