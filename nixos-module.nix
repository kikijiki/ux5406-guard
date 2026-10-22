self:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.ux5406-guard;
  inherit (lib) mkEnableOption mkOption mkIf types;
  attribute = "/sys/devices/pci0000:00/0000:00:04.0/tcc_offset_degree_celsius";
  guard = "${cfg.package}/bin/ux5406-guard";
  flags = lib.escapeShellArgs [
    "--hold-temp" (toString cfg.holdTemp)
    "--step-up" (toString cfg.stepUp)
    "--max-offset" (toString cfg.maxOffset)
    "--interval" (toString cfg.interval)
    "--settle" (toString cfg.settle)
    "--probe-interval" (toString cfg.probeInterval)
  ];
in
{
  options.services.ux5406-guard = {
    enable = mkEnableOption "the UX5406SA thermal guard (raises the CPU thermal offset on the EC's 0x6d warning)";
    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The ux5406-guard package.";
    };
    holdTemp = mkOption {
      type = types.int;
      default = 85;
      description = "Package temperature (C) the offset is raised to reach after an event. Not calibrated for this model; see the README.";
    };
    stepUp = mkOption { type = types.int; default = 2; description = "Degrees added per raise."; };
    maxOffset = mkOption { type = types.int; default = 30; description = "Largest offset the guard writes."; };
    interval = mkOption { type = types.int; default = 4; description = "Seconds between control steps."; };
    settle = mkOption { type = types.int; default = 20; description = "Seconds at or below holdTemp before the alert counts as cleared."; };
    probeInterval = mkOption { type = types.int; default = 180; description = "Seconds without an event before the offset is lowered by one degree."; };
  };

  config = mkIf cfg.enable {
    systemd.services.ux5406-guard = {
      description = "CPU thermal-offset guard for the UX5406SA EC thermal warning (0x6d)";
      wantedBy = [ "multi-user.target" ];
      after = [ "thermald.service" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${guard} run ${flags}";
        # A stopped guard restores the offset itself; a killed one leaves a
        # note in /run/ux5406-guard, which this reads.
        ExecStopPost = "${guard} restore";
        Restart = "on-failure";
        RestartSec = 30;
        # Exit 78: wrong product or bad options. Restarting cannot help.
        RestartPreventExitStatus = 78;

        RuntimeDirectory = "ux5406-guard";
        RuntimeDirectoryMode = "0700";
        RuntimeDirectoryPreserve = "yes";
        UMask = "0077";

        # /sys is read-only except the one attribute.
        ProtectKernelTunables = true;
        ReadWritePaths = [ attribute ];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateNetwork = true;
        IPAddressDeny = "any";
        RestrictAddressFamilies = "none";
        # The kernel log is read from /dev/kmsg, so the device stays visible.
        DevicePolicy = "closed";
        DeviceAllow = [ "/dev/kmsg r" ];
        CapabilityBoundingSet = [ "CAP_SYSLOG" ];
        AmbientCapabilities = [ ];
        NoNewPrivileges = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RemoveIPC = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" "~@resources" ];
      };
    };
  };
}
