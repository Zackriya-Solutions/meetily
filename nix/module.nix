{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.meetily;
in
{
  options.programs.meetily = {
    enable = lib.mkEnableOption "Meetily, privacy-first AI meeting assistant";

    package = lib.mkPackageOption pkgs "meetily" {
      default = [ "meetily" ];
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
