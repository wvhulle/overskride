bindruneFlake:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  bindruneSettings = config.services.bindrune;
in
{
  options.services.bindrune = {
    enable = lib.mkEnableOption "Bindrune in the background for every graphical user session";
    package = lib.mkOption {
      type = lib.types.package;
      default = bindruneFlake.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "bindrune.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Bindrune package to run.";
    };
  };

  config = lib.mkIf bindruneSettings.enable {
    environment.systemPackages = [ bindruneSettings.package ];
    systemd.packages = [ bindruneSettings.package ];
    systemd.user.services.bindrune.wantedBy = [ "graphical-session.target" ];
  };
}
