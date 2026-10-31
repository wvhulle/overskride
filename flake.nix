{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      forEachSystem =
        buildForSystem:
        nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ] (
          system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            };
          in
          buildForSystem {
            inherit pkgs;
            craneLib = (crane.mkLib pkgs).overrideToolchain (
              toolchainPackages: toolchainPackages.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
            );
          }
        );

      overskrideBuild =
        { pkgs, craneLib }:
        let
          inherit (pkgs) lib;

          applicationId = "io.github.kaii_lb.Overskride";

          rustSource = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./overskride-ui/src/icons
              ./data/${applicationId}.gschema.xml
            ];
          };

          commonArgs = {
            src = rustSource;
            pname = "overskride";
            version = (lib.importTOML ./overskride-ui/Cargo.toml).package.version;
            cargoExtraArgs = "--locked --package overskride-ui";
            strictDeps = true;
            nativeBuildInputs = with pkgs; [
              pkg-config
              glib
              libxml2
            ];
            buildInputs = with pkgs; [
              gtk4
              libadwaita
              dbus
              systemdLibs
              libpulseaudio
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          overskrideBinary = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              doCheck = false;
            }
          );

          packageMeta = {
            description = "Bluetooth and OBEX client for GNOME";
            mainProgram = "overskride";
            license = lib.licenses.gpl3Plus;
            platforms = lib.platforms.linux;
          };

          overskrideApplication = pkgs.stdenv.mkDerivation {
            pname = "overskride-application";
            inherit (commonArgs) version;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Makefile
                ./data
              ];
            };
            nativeBuildInputs = [
              pkgs.wrapGAppsHook4
              pkgs.glib
            ];
            buildInputs = commonArgs.buildInputs;
            dontBuild = true;
            installPhase = ''
              runHook preInstall
              install -Dm755 ${overskrideBinary}/bin/overskride $out/bin/overskride
              make install-metainfo install-schema install-icons install-systemd-unit DESTDIR=$out PREFIX= BINARY_DIRECTORY=$out/bin
              glib-compile-schemas --strict $out/share/glib-2.0/schemas
              runHook postInstall
            '';
            meta = packageMeta;
          };

          desktopEntry = pkgs.makeDesktopItem {
            name = applicationId;
            desktopName = "Overskride";
            comment = "A simple yet powerful bluetooth app!";
            exec = lib.getExe overskrideApplication;
            icon = applicationId;
            categories = [ "Utility" ];
            keywords = [
              "blue"
              "bluetooth"
              "transfer"
              "send"
              "audio"
              "share"
            ];
            startupNotify = true;
            terminal = false;
          };

          overskride = pkgs.symlinkJoin {
            pname = "overskride";
            inherit (commonArgs) version;
            paths = [
              overskrideApplication
              desktopEntry
            ];
            meta = packageMeta;
          };
        in
        {
          inherit
            commonArgs
            cargoArtifacts
            overskride
            ;
        };
    in
    {
      overlays.default = final: _previous: {
        overskride = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      homeManagerModules.default = import ./nix/home-manager-module.nix self;

      nixosModules.default = import ./nix/nixos-module.nix self;

      packages = forEachSystem (
        systemContext:
        let
          builtOverskride = overskrideBuild systemContext;
        in
        {
          default = builtOverskride.overskride;
          inherit (builtOverskride) overskride;
        }
      );

      checks = forEachSystem (
        { craneLib, pkgs }@systemContext:
        let
          builtOverskride = overskrideBuild systemContext;
          backgroundServiceSystem = nixpkgs.lib.nixosSystem {
            inherit (pkgs.stdenv.hostPlatform) system;
            modules = [
              self.nixosModules.default
              {
                services.overskride.enable = true;
                boot.loader.grub.enable = false;
                fileSystems."/".device = "nodev";
                system.stateVersion = "25.11";
              }
            ];
          };
        in
        {
          nixos-background-service =
            backgroundServiceSystem.config.systemd.user.units."overskride.service".unit;
          tests = craneLib.cargoTest (
            builtOverskride.commonArgs // { inherit (builtOverskride) cargoArtifacts; }
          );
          clippy = craneLib.cargoClippy (
            builtOverskride.commonArgs
            // {
              inherit (builtOverskride) cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets";
            }
          );
        }
      );

      formatter = forEachSystem ({ pkgs, ... }: pkgs.nixfmt);

      devShells = forEachSystem (
        { pkgs, craneLib }@systemContext:
        let
          builtOverskride = overskrideBuild systemContext;
        in
        {
          default = craneLib.devShell {
            packages =
              builtOverskride.commonArgs.nativeBuildInputs
              ++ builtOverskride.commonArgs.buildInputs
              ++ [
                pkgs.desktop-file-utils
                pkgs.appstream
              ];
          };
        }
      );
    };
}
