{
  pkgs,
  craneLib,
  commonArgs,
  cargoArtifacts,
  bindrune,
}:
let
  screenshotFontDirectories = pkgs.makeFontsConf {
    fontDirectories = [ pkgs.adwaita-fonts ];
  };

  screenshotFonts = pkgs.writeText "screenshot-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <include>${screenshotFontDirectories}</include>
      <alias binding="same"><family>Sans</family><prefer><family>Adwaita Sans</family></prefer></alias>
      <alias binding="same"><family>sans-serif</family><prefer><family>Adwaita Sans</family></prefer></alias>
      <alias binding="same"><family>system-ui</family><prefer><family>Adwaita Sans</family></prefer></alias>
      <alias binding="same"><family>Monospace</family><prefer><family>Adwaita Mono</family></prefer></alias>
      <alias binding="same"><family>monospace</family><prefer><family>Adwaita Mono</family></prefer></alias>
    </fontconfig>
  '';

  screenshotShowcaseBinary = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      pname = "bindrune-screenshot-showcase";
      cargoExtraArgs = "--locked --package bindrune-screenshot-showcase";
      doCheck = false;
    }
  );

  renderReadmeScreenshots = pkgs.symlinkJoin {
    name = "render-readme-screenshots";
    paths = [ screenshotShowcaseBinary ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/render-readme-screenshots \
        --prefix PATH : ${
          pkgs.lib.makeBinPath [
            bindrune
            pkgs.dbus
            pkgs.grim
            pkgs.sway
          ]
        } \
        --set FONTCONFIG_FILE ${screenshotFonts} \
        --prefix XDG_DATA_DIRS : ${
          pkgs.lib.makeSearchPath "share" [
            pkgs.adwaita-icon-theme
            pkgs.hicolor-icon-theme
          ]
        }:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}
    '';
    meta.mainProgram = "render-readme-screenshots";
  };

  readmeScreenshots = pkgs.runCommand "readme-screenshots" { } ''
    ${pkgs.lib.getExe renderReadmeScreenshots} $out/screenshots
  '';
in
{
  inherit renderReadmeScreenshots readmeScreenshots;
}
