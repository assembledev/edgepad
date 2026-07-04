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
  udevRules =
    if cfg.accessMode == "uaccess" then
      ''
        SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_TOUCHPAD}=="1", TAG+="uaccess"
        SUBSYSTEM=="misc", KERNEL=="uinput", TAG+="uaccess", OPTIONS+="static_node=uinput"
      ''
    else
      ''
        KERNEL=="uinput", GROUP="${cfg.inputGroup}", MODE="0660", OPTIONS+="static_node=uinput"
      '';
  udevRulesPackage = pkgs.writeTextFile {
    name = "edgepad-udev-rules";
    destination = "/lib/udev/rules.d/70-edgepad.rules";
    text = udevRules;
  };
in
{
  options.services.edgepad = {
    enable = lib.mkEnableOption "system support for the edgepad user-session daemon";

    accessMode = lib.mkOption {
      type = lib.types.enum [
        "uaccess"
        "group"
      ];
      default = "uaccess";
      description = ''
        Device access strategy. `uaccess` grants the active local seat user access through
        systemd-logind ACLs. `group` grants configured users persistent access through
        the input group and requires a new login session after group membership changes.
      '';
    };

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
        Users allowed to run edgepad against /dev/input and /dev/uinput when accessMode is `group`.
        Existing login sessions may need to be restarted before new group membership is visible.
      '';
    };

    inputGroup = lib.mkOption {
      type = lib.types.str;
      default = "input";
      description = "Group used for /dev/input and /dev/uinput access when accessMode is `group`.";
    };

    enableUdevRules = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install udev rules for the selected device access mode.";
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        environment.systemPackages = [ cfg.package ];
        boot.kernelModules = [ "uinput" ];
        services.udev.packages = lib.mkIf cfg.enableUdevRules [ udevRulesPackage ];
      }

      (lib.mkIf (cfg.accessMode == "group") {
        users.groups.${cfg.inputGroup} = { };
        users.users = lib.genAttrs cfg.users (_user: {
          extraGroups = [ cfg.inputGroup ];
        });
      })
    ]
  );
}
