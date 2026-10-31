# Bindrune

A Bluetooth and OBEX client for Linux desktops that is straight to the point,
DE/WM agnostic, and beautiful.

The name comes from the Bluetooth logo, a bindrune that joins the runes ᚼ and
ᛒ, the initials of Harald Bluetooth. Bindrune is a hard fork of
[Overskride](https://github.com/kaii-lb/overskride) by kaii.

![Screenshot](/assets/images/bindrune.png)

## What it does

- Lists every device the adapter knows or finds while scanning, sorted by
  connection, pairing, name and signal strength. Devices without a name are
  grouped by their guessed type.
- Shows pairing, bonding, trust, signal and connection as one timeline per
  device, with the reason when a step fails and the action that fixes it.
- Shows each adapter's hardware, its visibility for pairing and the name other
  devices see.
- Sends and receives files over OBEX, with an auto-accept choice per device.
- Switches audio profiles, reads battery levels and reports link quality for
  connected input devices.
- Keeps running in the background with a tray icon, so phones can still pair
  and send files while the window is closed.

## Missing

- Auto accept of services.

## Get started

- Install it with Nix, the NixOS or home-manager module, from source or as a
  Flatpak: [docs/installation.md](docs/installation.md).
- Work on it: [docs/development.md](docs/development.md) and
  [docs/style.md](docs/style.md).
