# Style

The rules the code in Bindrune follows. Setup, building and CI are in
[development.md](development.md).

- No comments, doc-comments or docstrings. Intent lives in names and call flow;
  code that cannot be made self-evident that way is restructured, not
  annotated.
- Names have two to four concrete keywords and spell words out
  (`pairing_visibility_group`, `DeviceConnectionHistories`). A type is named
  after its file: `sidebar_navigation_page.rs` holds `SidebarNavigationPage`.
- `bindrune-gtk/src/` is grouped by the region of the window a module draws.
  Code shared by several regions, such as device icons and badges, lives in a
  top-level `shared_*` module instead of one region importing from another.
- `bindrune-bluez/src/` is grouped by BlueZ concept and never depends on GTK.
- A region that grows large is split into Relm4 child components, not into
  files of loose helper functions. A child owns its state, receives new
  snapshots through its `Input` and reports through its `Output`; parents do
  not reach into child widgets. Repeated rows are `FactoryComponent`s.
  Presentation logic that turns snapshots into text and view models stays as
  plain functions, so it is tested without GTK.
- Files stay between roughly 120 and 400 lines of code, not counting tests.
  Split a longer file at a real seam; merge a shorter one into its only user
  or into the module about the same concept.
- Show values as BlueZ and the kernel report them; do not reformat or
  capitalize them.
- Formatting is nightly rustfmt with imports grouped by crate (`nix fmt`).
