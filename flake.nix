{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      overlays.default = final: previous: {
        overskride = previous.overskride.overrideAttrs (previousAttributes: {
          version = "${previousAttributes.version}-pairing-timeline";
          src = self;
          cargoDeps = final.rustPlatform.fetchCargoVendor {
            src = self;
            hash = "sha256-Vhri7BqNbgWpX92ffQYpIAjJqnsJBaOpCk5UiVuOBz8=";
          };
        });
      };

      packages = forEachSystem (system: {
        default = (nixpkgs.legacyPackages.${system}.extend self.overlays.default).overskride;
      });

      devShells = forEachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          overskride-dev = pkgs.writeShellApplication {
            name = "overskride-dev";
            text = ''
              project_root=$(pwd)
              while [ ! -f "$project_root/meson.build" ] || [ ! -f "$project_root/Cargo.toml" ]; do
                if [ "$project_root" = "/" ]; then
                  echo "overskride-dev: run inside the overskride checkout" >&2
                  exit 1
                fi
                project_root=$(dirname "$project_root")
              done

              build_directory="$project_root/build"
              if [ ! -f "$build_directory/build.ninja" ]; then
                meson setup "$build_directory" "$project_root"
              fi
              meson compile -C "$build_directory"
              RUST_BACKTRACE=1 meson devenv -C "$build_directory" ./src/overskride "$@"
            '';
          };
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ pkgs.overskride ];
            packages = [
              overskride-dev
              pkgs.rust-analyzer
              pkgs.clippy
              pkgs.rustfmt
              pkgs.libxml2
            ];
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        }
      );
    };
}
