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
    enable = lib.mkEnableOption "Overskride in the background as Bluetooth pairing agent and file receiver";
    package = lib.mkOption {
      type = lib.types.package;
      default = overskrideFlake.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "overskride.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Overskride package to run.";
    };
  };

  config = lib.mkIf overskrideSettings.enable {
    home.packages = [ overskrideSettings.package ];
    systemd.user.services.overskride = {
      Unit = {
        Description = "Overskride Bluetooth pairing agent and file receiver";
        PartOf = [ "graphical-session.target" ];
        After = [
          "graphical-session.target"
          "bluetooth.target"
        ];
        Requisite = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${lib.getExe overskrideSettings.package} --background";
        Environment = [ "OVERSKRIDE_SYSTEMD_SERVICE=1" ];
        Restart = "on-failure";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
