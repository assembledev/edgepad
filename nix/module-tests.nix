{
  self,
  lib,
  pkgs,
}:

let
  nixosEval = lib.evalModules {
    specialArgs = {
      inherit pkgs;
    };

    modules = [
      (import ./nixos-module.nix self)
      {
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
          services.udev.extraRules = lib.mkOption {
            type = lib.types.lines;
            default = "";
          };
        };

        config.services.edgepad = {
          enable = true;
          users = [ "alice" ];
        };
      }
    ];
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
          systemd.user.services = lib.mkOption {
            type = lib.types.attrsOf lib.types.attrs;
            default = { };
          };
        };

        config.services.edgepad = {
          enable = true;
          device = "auto";
          edgeWidth = 0.1;
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
          ];
        };
      }
    ];
  };

  homeConfigFile = homeEval.config.xdg.configFile."edgepad/edgepad.toml".source;
  homeService = homeEval.config.systemd.user.services.edgepad;

  checked =
    assert lib.elem "uinput" nixosEval.config.boot.kernelModules;
    assert lib.hasAttr "input" nixosEval.config.users.groups;
    assert lib.elem "input" nixosEval.config.users.users.alice.extraGroups;
    assert lib.hasInfix ''KERNEL=="uinput"'' nixosEval.config.services.udev.extraRules;
    assert lib.hasInfix "daemon --config" (
      builtins.unsafeDiscardStringContext homeService.Service.ExecStart
    );
    pkgs.runCommand "edgepad-module-tests" { } ''
      set -eu
      grep -F 'device = "auto"' ${homeConfigFile}
      grep -F 'edge_width = 0.1' ${homeConfigFile}
      grep -F '[[gestures]]' ${homeConfigFile}
      grep -F 'zone = "right"' ${homeConfigFile}
      grep -F 'direction = "down"' ${homeConfigFile}
      grep -F 'action = ["notify-send", "edgepad", "right-down"]' ${homeConfigFile}
      grep -F 'zone = "top"' ${homeConfigFile}
      grep -F 'direction = "right"' ${homeConfigFile}
      grep -F 'action = ["notify-send", "edgepad", "top-right"]' ${homeConfigFile}
      touch $out
    '';
in
checked
