{
  self,
  lib,
  pkgs,
}:

let
  nixosBaseModule = {
    options = {
      environment.systemPackages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
      };
      boot.kernelModules = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
      };
      users.groups = lib.mkOption {
        type = lib.types.attrsOf lib.types.attrs;
        default = { };
      };
      users.users = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule {
            options.extraGroups = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
            };
          }
        );
        default = { };
      };
      services.udev.packages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
      };
    };
  };

  evalNixosModule =
    edgepadConfig:
    lib.evalModules {
      specialArgs = {
        inherit pkgs;
      };

      modules = [
        (import ./nixos-module.nix self)
        nixosBaseModule
        {
          config.services.edgepad = edgepadConfig;
        }
      ];
    };

  nixosUaccessEval = evalNixosModule {
    enable = true;
  };

  nixosGroupEval = evalNixosModule {
    enable = true;
    accessMode = "group";
    users = [ "alice" ];
  };

  homeEval = lib.evalModules {
    specialArgs = {
      inherit pkgs;
    };

    modules = [
      (import ./home-manager-module.nix self)
      {
        options = {
          home.packages = lib.mkOption {
            type = lib.types.listOf lib.types.package;
            default = [ ];
          };
          xdg.configFile = lib.mkOption {
            type = lib.types.attrsOf lib.types.attrs;
            default = { };
          };
          xdg.configHome = lib.mkOption {
            type = lib.types.str;
            default = "/home/alice/.config";
          };
          systemd.user.services = lib.mkOption {
            type = lib.types.attrsOf lib.types.attrs;
            default = { };
          };
          assertions = lib.mkOption {
            type = lib.types.listOf lib.types.attrs;
            default = [ ];
          };
        };

        config.services.edgepad = {
          enable = true;
          device = "auto";
          edgeWidth = 0.1;
          edgeWidths = {
            left = 0.08;
            top = 0.12;
          };
          tapMinDurationMs = 90;
          tapMaxDurationMs = 220;
          doubleTapTimeoutMs = 320;
          doubleTapMaxDistance = 0.05;
          swipeMinDistance = 0.03;
          gestures = [
            {
              zone = "right";
              direction = "down";
              action = [
                "notify-send"
                "edgepad"
                "right-down"
              ];
            }
            {
              zone = "top";
              direction = "right";
              action = [
                "notify-send"
                "edgepad"
                "top-right"
              ];
            }
            {
              zone = "top";
              direction = "double-tap";
              action = [
                "notify-send"
                "edgepad"
                "top-double-tap"
              ];
            }
          ];
          sliders = [
            {
              zone = "left";
              up = [
                "pamixer"
                "-i"
                "3"
              ];
              down = [
                "pamixer"
                "-d"
                "3"
              ];
            }
          ];
        };
      }
    ];
  };

  homeConfigFile = homeEval.config.xdg.configFile."edgepad/edgepad.toml".source;
  homeService = homeEval.config.systemd.user.services.edgepad;
  homeExecStart = builtins.unsafeDiscardStringContext homeService.Service.ExecStart;
  uaccessUdevRulesPackage = lib.head nixosUaccessEval.config.services.udev.packages;
  groupUdevRulesPackage = lib.head nixosGroupEval.config.services.udev.packages;

  checked =
    assert lib.elem "uinput" nixosUaccessEval.config.boot.kernelModules;
    assert lib.length nixosUaccessEval.config.services.udev.packages == 1;
    assert lib.elem "uinput" nixosGroupEval.config.boot.kernelModules;
    assert lib.hasAttr "input" nixosGroupEval.config.users.groups;
    assert lib.elem "input" nixosGroupEval.config.users.users.alice.extraGroups;
    assert lib.length nixosGroupEval.config.services.udev.packages == 1;
    assert lib.hasInfix "daemon --config /home/alice/.config/edgepad/edgepad.toml --device auto"
      homeExecStart;
    assert lib.hasInfix "kill -HUP $MAINPID" (
      builtins.unsafeDiscardStringContext homeService.Service.ExecReload
    );
    assert homeService.Unit."X-Reload-Triggers" == [ homeConfigFile ];
    assert homeService.Service.Type == "notify";
    assert homeService.Service.NotifyAccess == "main";
    assert homeService.Service.TimeoutStartSec == "45s";
    assert lib.all (entry: entry.assertion) homeEval.config.assertions;
    pkgs.runCommand "edgepad-module-tests" { } ''
      set -eu
      grep -F 'device = "auto"' ${homeConfigFile}
      grep -F 'edge_width = 0.1' ${homeConfigFile}
      grep -F 'left_edge_width = 0.08' ${homeConfigFile}
      grep -F 'top_edge_width = 0.12' ${homeConfigFile}
      ! grep -F 'right_edge_width' ${homeConfigFile}
      ! grep -F 'bottom_edge_width' ${homeConfigFile}
      grep -F 'tap_min_duration_ms = 90' ${homeConfigFile}
      grep -F 'tap_max_duration_ms = 220' ${homeConfigFile}
      grep -F 'double_tap_timeout_ms = 320' ${homeConfigFile}
      grep -F 'double_tap_max_distance = 0.05' ${homeConfigFile}
      grep -F 'swipe_min_distance = 0.03' ${homeConfigFile}
      grep -F '[[gestures]]' ${homeConfigFile}
      grep -F 'zone = "right"' ${homeConfigFile}
      grep -F 'direction = "down"' ${homeConfigFile}
      grep -F 'action = ["notify-send", "edgepad", "right-down"]' ${homeConfigFile}
      grep -F 'zone = "top"' ${homeConfigFile}
      grep -F 'direction = "right"' ${homeConfigFile}
      grep -F 'action = ["notify-send", "edgepad", "top-right"]' ${homeConfigFile}
      grep -F 'direction = "double-tap"' ${homeConfigFile}
      grep -F 'action = ["notify-send", "edgepad", "top-double-tap"]' ${homeConfigFile}
      grep -F '[[sliders]]' ${homeConfigFile}
      grep -F 'zone = "left"' ${homeConfigFile}
      grep -F 'step = 0.04' ${homeConfigFile}
      grep -F 'up = ["pamixer", "-i", "3"]' ${homeConfigFile}
      grep -F 'down = ["pamixer", "-d", "3"]' ${homeConfigFile}
      grep -F 'ENV{ID_INPUT_TOUCHPAD}=="1"' ${uaccessUdevRulesPackage}/lib/udev/rules.d/70-edgepad.rules
      grep -F 'TAG+="uaccess"' ${uaccessUdevRulesPackage}/lib/udev/rules.d/70-edgepad.rules
      grep -F 'KERNEL=="uinput"' ${uaccessUdevRulesPackage}/lib/udev/rules.d/70-edgepad.rules
      grep -F 'GROUP="input"' ${groupUdevRulesPackage}/lib/udev/rules.d/70-edgepad.rules
      touch $out
    '';
in
checked
