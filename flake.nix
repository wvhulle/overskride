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

      nightlyRustfmt = pkgs: pkgs.rust-bin.nightly."2026-07-19".rustfmt;

      treefmtWithNightlyRustfmt =
        pkgs:
        pkgs.writeShellApplication {
          name = "treefmt";
          runtimeInputs = [
            pkgs.treefmt
            pkgs.nixfmt
            (nightlyRustfmt pkgs)
          ];
          text = ''
            exec treefmt "$@"
          '';
        };

      bindruneBuild =
        { pkgs, craneLib }:
        let
          inherit (pkgs) lib;

          applicationId = "page.codeberg.wvhulle.Bindrune";

          rustSource = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./bindrune-gtk/src/icons
              ./data/${applicationId}.gschema.xml
            ];
          };

          commonArgs = {
            src = rustSource;
            pname = "bindrune";
            version = (lib.importTOML ./bindrune-gtk/Cargo.toml).package.version;
            cargoExtraArgs = "--locked --package bindrune-gtk";
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

          bindruneBinary = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              doCheck = false;
            }
          );

          packageMeta = {
            description = "Bluetooth and OBEX client for GNOME";
            mainProgram = "bindrune";
            license = lib.licenses.gpl3Plus;
            platforms = lib.platforms.linux;
          };

          bindruneApplication = pkgs.stdenv.mkDerivation {
            pname = "bindrune-application";
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
              install -Dm755 ${bindruneBinary}/bin/bindrune $out/bin/bindrune
              make install-metainfo install-schema install-icons install-systemd-unit DESTDIR=$out PREFIX= BINARY_DIRECTORY=$out/bin
              glib-compile-schemas --strict $out/share/glib-2.0/schemas
              runHook postInstall
            '';
            meta = packageMeta;
          };

          desktopEntry = pkgs.makeDesktopItem {
            name = applicationId;
            desktopName = "Bindrune";
            comment = "A simple yet powerful bluetooth app!";
            exec = lib.getExe bindruneApplication;
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

          bindrune = pkgs.symlinkJoin {
            pname = "bindrune";
            inherit (commonArgs) version;
            paths = [
              bindruneApplication
              desktopEntry
            ];
            meta = packageMeta;
          };
        in
        {
          inherit
            commonArgs
            cargoArtifacts
            bindrune
            ;
        };
    in
    {
      homeManagerModules.default = import ./nix/home-manager-module.nix self;

      nixosModules.default = import ./nix/nixos-module.nix self;

      packages = forEachSystem (
        systemContext:
        let
          builtBindrune = bindruneBuild systemContext;
          readmeScreenshotRendering = import ./nix/readme-screenshots.nix {
            inherit (systemContext) pkgs craneLib;
            inherit (builtBindrune) bindrune commonArgs cargoArtifacts;
          };
        in
        {
          default = builtBindrune.bindrune;
          inherit (builtBindrune) bindrune;
          render-readme-screenshots = readmeScreenshotRendering.renderReadmeScreenshots;
          readme-screenshots = readmeScreenshotRendering.readmeScreenshots;
        }
      );

      checks = forEachSystem (
        { craneLib, pkgs }@systemContext:
        let
          builtBindrune = bindruneBuild systemContext;
          backgroundServiceSystem = nixpkgs.lib.nixosSystem {
            inherit (pkgs.stdenv.hostPlatform) system;
            modules = [
              self.nixosModules.default
              {
                services.bindrune.enable = true;
                boot.loader.grub.enable = false;
                fileSystems."/".device = "nodev";
                system.stateVersion = "25.11";
              }
            ];
          };
        in
        {
          formatting =
            pkgs.runCommand "formatting-check"
              {
                nativeBuildInputs = [ (treefmtWithNightlyRustfmt pkgs) ];
              }
              ''
                cp -r ${./.} source
                chmod -R u+w source
                cd source
                treefmt --ci --tree-root .
                touch $out
              '';
          nixos-background-service =
            backgroundServiceSystem.config.systemd.user.units."bindrune.service".unit;
          tests = craneLib.cargoTest (
            builtBindrune.commonArgs // { inherit (builtBindrune) cargoArtifacts; }
          );
          clippy = craneLib.cargoClippy (
            builtBindrune.commonArgs
            // {
              inherit (builtBindrune) cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets";
            }
          );
        }
      );

      formatter = forEachSystem ({ pkgs, ... }: treefmtWithNightlyRustfmt pkgs);

      devShells = forEachSystem (
        { pkgs, craneLib }@systemContext:
        let
          builtBindrune = bindruneBuild systemContext;
        in
        {
          default = craneLib.devShell {
            packages =
              builtBindrune.commonArgs.nativeBuildInputs
              ++ builtBindrune.commonArgs.buildInputs
              ++ [
                (treefmtWithNightlyRustfmt pkgs)
                (nightlyRustfmt pkgs)
                pkgs.nixfmt
                pkgs.desktop-file-utils
                pkgs.appstream
              ];
          };
        }
      );
    };
}
