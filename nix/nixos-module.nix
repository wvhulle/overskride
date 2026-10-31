overskrideFlake:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  overskrideSettings = config.services.overskride;
in
{
  options.services.overskride = {
    enable = lib.mkEnableOption "Overskride in the background for every graphical user session";
    package = lib.mkOption {
      type = lib.types.package;
      default = overskrideFlake.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "overskride.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Overskride package to run.";
    };
  };

  config = lib.mkIf overskrideSettings.enable {
    environment.systemPackages = [ overskrideSettings.package ];
    systemd.packages = [ overskrideSettings.package ];
    systemd.user.services.overskride.wantedBy = [ "graphical-session.target" ];
  };
}
