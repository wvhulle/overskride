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
    enable = lib.mkEnableOption "Bindrune in the background as Bluetooth pairing agent and file receiver";
    package = lib.mkOption {
      type = lib.types.package;
      default = bindruneFlake.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "bindrune.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Bindrune package to run.";
    };
  };

  config = lib.mkIf bindruneSettings.enable {
    home.packages = [ bindruneSettings.package ];
    systemd.user.services.bindrune = {
      Unit = {
        Description = "Bindrune Bluetooth pairing agent and file receiver";
        PartOf = [ "graphical-session.target" ];
        After = [
          "graphical-session.target"
          "bluetooth.target"
        ];
        Requisite = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${lib.getExe bindruneSettings.package} --background";
        Environment = [ "BINDRUNE_SYSTEMD_SERVICE=1" ];
        Restart = "on-failure";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
