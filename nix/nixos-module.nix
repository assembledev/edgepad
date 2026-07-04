self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.edgepad;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.edgepad;
in
{
  options.services.edgepad = {
    enable = lib.mkEnableOption "system support for the edgepad user-session daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "inputs.edgepad.packages.\${pkgs.stdenv.hostPlatform.system}.edgepad";
      description = "edgepad package to make available system-wide.";
    };

    users = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "alice" ];
      description = ''
        Users allowed to run edgepad against /dev/input and /dev/uinput through the input group.
        Existing login sessions may need to be restarted before new group membership is visible.
      '';
    };

    inputGroup = lib.mkOption {
      type = lib.types.str;
      default = "input";
      description = "Group used for /dev/input and /dev/uinput access.";
    };

    enableUinputRule = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install a udev rule that exposes /dev/uinput to the configured input group.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    boot.kernelModules = [ "uinput" ];

    users.groups.${cfg.inputGroup} = { };
    users.users = lib.genAttrs cfg.users (_user: {
      extraGroups = [ cfg.inputGroup ];
    });

    services.udev.extraRules = lib.mkIf cfg.enableUinputRule ''
      KERNEL=="uinput", GROUP="${cfg.inputGroup}", MODE="0660", OPTIONS+="static_node=uinput"
    '';
  };
}
