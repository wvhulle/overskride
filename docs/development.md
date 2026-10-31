# Development

Setup and workflow for working on Bindrune. The rules the code follows are in
[style.md](style.md).

## Workspace

Work inside the Nix devshell (`nix develop`, or `direnv allow`). The
repository is a cargo workspace:

- `bindrune-bluez/` wraps BlueZ, obexd, udev and PulseAudio. It knows nothing
  about GTK and exposes snapshots, change streams and commands, grouped by
  BlueZ concept: `bluetooth_adapter/`, `remote_device/`, `device_pairing/`,
  `link_quality/`, `obex_file_transfer/`.
- `bindrune-gtk/` is the Relm4 application (binary `bindrune`), grouped by the
  region of the window each module draws: `sidebar_navigation/`,
  `device_detail/`, `adapter_detail/`, `header_transfer_popover/`,
  `incoming_request_prompts/`. `background_application_lifecycle/` holds what
  draws nothing on screen: the window shell, tray menu, stored preferences and
  login autostart.

## Component tree

```mermaid
flowchart TB
  main_window_shell["MainWindowShell"]
  split_view["SidebarContentSplitView"]
  sidebar_page["SidebarNavigationPage"]
  scan_row["DiscoveryScanToggleRow"]
  device_rows["SidebarDeviceRows"]
  adapter_rows["SidebarAdapterRow factory"]
  device_page["DeviceDetailPage"]
  timeline["PairingStepTimeline"]
  step_rows["PairingStepRow factory"]
  bond_rows["AdapterBondRow factory"]
  device_groups["DeviceNameGroup, AudioProfileSelectorGroup,<br>FileTransferGroup, DeviceCapabilityOverview,<br>DeviceIdentityGroup"]
  adapter_page["AdapterDetailPage"]
  adapter_groups["PairingVisibilityGroup, AdapterHardwareGroup"]
  preferences_page["ApplicationPreferencesPage"]
  transfer_popover["FileTransferPopover"]
  progress_rows["FileTransferProgressRow factory"]
  prompt_queue["IncomingRequestPromptQueue"]
  tray_menu["SystemTrayMenu"]

  main_window_shell --> split_view
  main_window_shell --> tray_menu
  split_view --> sidebar_page
  sidebar_page --> scan_row & device_rows & adapter_rows
  split_view --> device_page & adapter_page & preferences_page
  device_page --> timeline & device_groups
  timeline --> step_rows & bond_rows
  adapter_page --> adapter_groups
  split_view --> transfer_popover --> progress_rows
  split_view --> prompt_queue
```

Each child owns its state and talks to its parent only through its `Input`
and `Output` messages.

## Building and checking

```bash
cargo run -p bindrune-gtk     # run from the checkout
cargo test --workspace
cargo clippy --workspace --all-targets
make check                    # tests plus desktop file, metainfo and schema validation
nix flake check               # tests, clippy, formatting and the NixOS service unit
```

Formatting uses nightly rustfmt, pinned in `flake.nix`, so imports are grouped
by crate. The build itself stays on the stable toolchain from
`rust-toolchain.toml`. Run `nix fmt` (treefmt with rustfmt and nixfmt), or
`cargo +nightly fmt` without Nix.

## CI

Woodpecker CI (`.woodpecker/checks.yaml`) builds the flake `checks` on pull
requests and on pushes to `main`: tests and clippy (through the `wvhulle`
Cachix cache on `main`), formatting, and the NixOS background service unit. On
pull requests it also checks the commits against Conventional Commits
(locally: `nix run --inputs-from . nixpkgs#convco -- check main..HEAD`).
