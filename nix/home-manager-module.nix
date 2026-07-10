self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.edgepad;
  toml = pkgs.formats.toml { };
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.edgepad;
  commandActionType = lib.types.nonEmptyListOf lib.types.str;

  gestureType = lib.types.submodule {
    options = {
      zone = lib.mkOption {
        type = lib.types.enum [
          "left"
          "right"
          "top"
          "bottom"
        ];
        description = "Edge zone that must claim the gesture.";
      };

      direction = lib.mkOption {
        type = lib.types.enum [
          "up"
          "down"
          "left"
          "right"
          "tap"
        ];
        description = "Recognized gesture direction.";
      };

      action = lib.mkOption {
        type = commandActionType;
        example = [
          "notify-send"
          "edgepad"
          "top-right"
        ];
        description = "Command argv to run for the gesture. The command is not run through a shell.";
      };
    };
  };

  sliderActionOption =
    direction:
    lib.mkOption {
      type = lib.types.nullOr commandActionType;
      default = null;
      example = [
        "notify-send"
        "edgepad"
        direction
      ];
      description = "Command argv to run for the ${direction} slider step.";
    };

  sliderType = lib.types.submodule {
    options = {
      zone = lib.mkOption {
        type = lib.types.enum [
          "left"
          "right"
          "top"
          "bottom"
        ];
        description = "Edge zone that owns this continuous slider.";
      };

      step = lib.mkOption {
        type = lib.types.float;
        default = 0.04;
        apply =
          value:
          if value > 0.0 && value <= 1.0 then
            value
          else
            throw "services.edgepad.sliders.*.step must be > 0 and <= 1";
        description = "Normalized touchpad movement required to emit one slider step.";
      };

      up = sliderActionOption "up";
      down = sliderActionOption "down";
      left = sliderActionOption "left";
      right = sliderActionOption "right";
    };
  };

  sliderToToml =
    slider:
    {
      inherit (slider) zone step;
    }
    // lib.optionalAttrs (slider.up != null) { inherit (slider) up; }
    // lib.optionalAttrs (slider.down != null) { inherit (slider) down; }
    // lib.optionalAttrs (slider.left != null) { inherit (slider) left; }
    // lib.optionalAttrs (slider.right != null) { inherit (slider) right; };

  configFile = toml.generate "edgepad.toml" {
    device = cfg.device;
    edge_width = cfg.edgeWidth;
    tap_min_duration_ms = cfg.tapMinDurationMs;
    gestures = map (gesture: {
      inherit (gesture) zone direction action;
    }) cfg.gestures;
    sliders = map sliderToToml cfg.sliders;
  };
in
{
  options.services.edgepad = {
    enable = lib.mkEnableOption "edgepad touchpad edge gesture daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "inputs.edgepad.packages.\${pkgs.stdenv.hostPlatform.system}.edgepad";
      description = "edgepad package used by the user service.";
    };

    device = lib.mkOption {
      type = lib.types.str;
      default = "auto";
      example = "/dev/input/event7";
      description = "`auto` or an explicit /dev/input/eventX path.";
    };

    edgeWidth = lib.mkOption {
      type = lib.types.float;
      default = 0.10;
      apply =
        value:
        if value > 0.0 && value < 0.5 then
          value
        else
          throw "services.edgepad.edgeWidth must be > 0 and < 0.5";
      description = "Fractional edge width used by every edge zone.";
    };

    tapMinDurationMs = lib.mkOption {
      type = lib.types.ints.between 0 10000;
      default = 80;
      description = "Minimum edge contact duration in milliseconds required for a tap gesture.";
    };

    gestures = lib.mkOption {
      type = lib.types.listOf gestureType;
      default = [ ];
      example = lib.literalExpression ''
        [
          {
            zone = "right";
            direction = "down";
            action = [ "notify-send" "edgepad" "right-down" ];
          }
        ]
      '';
      description = "Gesture bindings written to edgepad's TOML config.";
    };

    sliders = lib.mkOption {
      type = lib.types.listOf sliderType;
      default = [ ];
      example = lib.literalExpression ''
        [
          {
            zone = "right";
            up = [ "brightnessctl" "set" "+5%" ];
            down = [ "brightnessctl" "set" "5%-" ];
          }
        ]
      '';
      description = "Continuous slider bindings written to edgepad's TOML config.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."edgepad/edgepad.toml".source = configFile;

    systemd.user.services.edgepad = {
      Unit = {
        Description = "edgepad touchpad edge gesture daemon";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };

      Service = {
        ExecStart = "${lib.getExe cfg.package} daemon --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "1s";
      };

      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
