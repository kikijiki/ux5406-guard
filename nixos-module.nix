self:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.ux5406-guard;
  inherit (lib) mkEnableOption mkOption mkIf types;
  limits = [
    "/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw"
    "/sys/devices/virtual/powercap/intel-rapl-mmio/intel-rapl-mmio:0/constraint_0_power_limit_uw"
  ];
  guard = "${cfg.package}/bin/ux5406-guard";
  flags = lib.escapeShellArgs [
    "--cap" (toString cfg.capWatts)
    "--safe" (toString cfg.safeWatts)
    "--hold" (toString cfg.holdSeconds)
  ];
in
{
  options.services.ux5406-guard = {
    enable = mkEnableOption "the UX5406SA power guard (caps package power while the lid is closed and after the EC's 0x6d warning)";
    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The ux5406-guard package.";
    };
    capWatts = mkOption {
      type = types.int;
      default = 13;
      description = "Package power (W) while the lid is closed. At most 14: 14 W ran 20 minutes at full load without an event, 16 W did not.";
    };
    safeWatts = mkOption { type = types.int; default = 9; description = "Package power (W) after an event."; };
    holdSeconds = mkOption { type = types.int; default = 300; description = "Seconds the safe level is kept after the last event."; };
  };

  config = mkIf cfg.enable {
    systemd.services.ux5406-guard = {
      description = "Package power guard for the UX5406SA EC thermal warning (0x6d)";
      wantedBy = [ "multi-user.target" ];
      after = [ "thermald.service" ];
      serviceConfig = {
        Type = "exec";
        ExecStart = "${guard} run ${flags}";
        # A stopped guard restores the limits itself; a killed one leaves a
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

        # /sys is read-only except the two limits.
        ProtectKernelTunables = true;
        ReadWritePaths = limits;
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
        # The lid state is read from /proc/acpi.
        ProcSubset = "all";
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
