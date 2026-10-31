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
              ./overskride-ui/src/overskride.gresource.xml
              ./overskride-ui/src/gtk
              ./overskride-ui/src/icons
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

          overskrideData = pkgs.stdenvNoCC.mkDerivation {
            pname = "overskride-data";
            inherit (commonArgs) version;
            src = lib.fileset.toSource {
              root = ./data;
              fileset = lib.fileset.unions [
                ./data/${applicationId}.desktop.in
                ./data/${applicationId}.appdata.xml.in
                ./data/${applicationId}.gschema.xml
                ./data/icons/hicolor
              ];
            };
            nativeBuildInputs = [
              pkgs.glib
              pkgs.desktop-file-utils
            ];
            installPhase = ''
              runHook preInstall
              install -Dm644 ${applicationId}.desktop.in $out/share/applications/${applicationId}.desktop
              desktop-file-validate $out/share/applications/${applicationId}.desktop
              install -Dm644 ${applicationId}.appdata.xml.in $out/share/metainfo/${applicationId}.metainfo.xml
              install -Dm644 icons/hicolor/scalable/apps/${applicationId}.svg \
                $out/share/icons/hicolor/scalable/apps/${applicationId}.svg
              install -Dm644 icons/hicolor/symbolic/apps/${applicationId}-symbolic.svg \
                $out/share/icons/hicolor/symbolic/apps/${applicationId}-symbolic.svg
              install -Dm644 ${applicationId}.gschema.xml $out/share/glib-2.0/schemas/${applicationId}.gschema.xml
              glib-compile-schemas --strict $out/share/glib-2.0/schemas
              runHook postInstall
            '';
          };

          runtimeEnvironment = {
            GSETTINGS_SCHEMA_DIR = pkgs.glib.getSchemaPath overskrideData;
          };

          runtimeWrapperFlags = lib.concatStringsSep " " (
            lib.mapAttrsToList (
              variableName: variableValue: "--set ${variableName} ${lib.escapeShellArg variableValue}"
            ) runtimeEnvironment
          );

          overskride = pkgs.stdenv.mkDerivation {
            pname = "overskride";
            inherit (commonArgs) version;
            dontUnpack = true;
            nativeBuildInputs = [ pkgs.wrapGAppsHook4 ];
            buildInputs = commonArgs.buildInputs;
            installPhase = ''
              runHook preInstall
              install -Dm755 ${overskrideBinary}/bin/overskride $out/bin/overskride
              mkdir -p $out/share
              cp -r ${overskrideData}/share/. $out/share/
              runHook postInstall
            '';
            preFixup = ''
              gappsWrapperArgs+=(${runtimeWrapperFlags})
            '';
            meta = {
              description = "Bluetooth and OBEX client for GNOME";
              mainProgram = "overskride";
              license = lib.licenses.gpl3Plus;
              platforms = lib.platforms.linux;
            };
          };
        in
        {
          inherit
            commonArgs
            cargoArtifacts
            overskride
            runtimeEnvironment
            ;
        };
    in
    {
      overlays.default = final: _previous: {
        overskride = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

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
        { craneLib, ... }@systemContext:
        let
          builtOverskride = overskrideBuild systemContext;
        in
        {
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
                pkgs.meson
                pkgs.ninja
                pkgs.desktop-file-utils
              ];
            env = builtOverskride.runtimeEnvironment;
          };
        }
      );
    };
}
