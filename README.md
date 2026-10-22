# ux5406-guard

Thermal guard for the ASUS Zenbook S 14 UX5406SA (Intel Lunar Lake). It sheds CPU
load so the embedded controller (EC) does not cut power when the lid is closed.
It never sleeps, suspends, hibernates or powers off the machine.

## Problem

With the lid closed and the CPU hot, the EC raises a thermal warning that the
kernel logs as `asus_wmi: Unknown key code 0x6d`. If the CPU stays hot the EC cuts
power 1.5 to 2.5 minutes later, leaving no trace in the journal. Windows hibernates
on this event; Linux ignores it.

Observations on one unit (BIOS UX5406SA.309, kernel 7.2): three power losses, each
with AC connected, lid closed and a sustained all-core load (cargo and nvcc builds).
The last event preceded the end of the journal by 1m34s to 2m17s. In one boot the
event fired six times, 4 to 6 minutes apart, without a loss, so it is a warning and
not always fatal. The charge cap (80 %), the BIOS and the `ucsi_acpi ...
GET_CURRENT_CAM command failed` message do not correlate with the losses; the
message also appears on boots that ended cleanly.

## Mechanism

On the event the guard raises the CPU thermal offset
(`/sys/bus/pci/devices/0000:00:04.0/tcc_offset_degree_celsius`), which lowers the
temperature the CPU throttles at (tjmax minus offset).

- Idle: reads `/dev/kmsg` (kernel records only, from the end). Writes nothing.
- Event: saves the current offset, then raises it by `step-up` every `interval`
  seconds while the package temperature is above `hold-temp`, up to `max-offset`.
- Hold: after `settle` seconds at or below `hold-temp` the offset is held. After
  `probe-interval` seconds without an event it is lowered one degree, repeatedly,
  until the original offset is back.
- An event while holding raises again.

Only the offset attribute is written. The guard refuses to run unless the DMI
product name contains `UX5406SA` (exit status 78, not restarted). It restores the
original offset on SIGTERM/SIGINT; after a crash or SIGKILL the unit's
`ExecStopPost` (`ux5406-guard restore`) or the next start does it from the note in
`/run/ux5406-guard`. Read and write failures are logged and retried; the service
keeps running.

## Limits

- It reduces heat. It does not identify the EC's condition.
- The first event of an episode is not prevented; the guard reacts to it.
- `hold-temp` is not calibrated for this model (below).
- No shutdown logic. The kernel thermal core's trips (103 and 110 C here) are the
  only backstop.

## Calibration

Measured on one unit: tjmax 100 C, default offset 8 (throttle at 92 C), PL1/PL2
37/40 W. Lid open, the package sat at 84 to 85 C under load and peaked at 93 C with
no event. Journals from lost boots contain no temperatures, so the temperature at
which the EC raises the event and the one at which it stops are unknown.
The default `hold-temp` of 85 C is conservative.

To calibrate, log package temperature (1 s resolution around events), package
power, and event timestamps. Needed:

1. Package temperature just before each event: set `hold-temp` a few degrees below.
2. Whether events repeat while the guard holds, and at what temperature they stop.
3. Package power at steady state, for the throughput cost.
4. For any power loss, the time from the last event to the end of the log.

## Usage

Settings (defaults): `--hold-temp 85` C, `--step-up 2`, `--max-offset 30`,
`--interval 4` s, `--settle 20` s, `--probe-interval 180` s (at least 60; longer
than the EC's delay before it raises the event). `ux5406-guard restore` writes
back a saved offset and exits.

NixOS:

```nix
inputs.ux5406-guard.url = "github:kikijiki/ux5406-guard";
# modules:
imports = [ inputs.ux5406-guard.nixosModules.default ];
services.ux5406-guard.enable = true;
# options: holdTemp stepUp maxOffset interval settle probeInterval
```

Log: `journalctl -u ux5406-guard`. Examples:

```
event: asus_wmi: Unknown key code 0x6d
91 C: above hold temperature: offset raised to 10
80 C: cooled, holding offset 16
no event: offset lowered to 15
no event at the original offset: released, offset 8
stopping: offset back to 8
error: cannot write /sys/...: Read-only file system
```

Remove: disable the option and rebuild. Stopping the unit restores the offset.

## Related

- thermald runs in polling mode on this model with no usable tables (empty ACPI
  `_TRT`) and does not touch the offset. Its adaptive mode (Intel DPTF)
  could set power limits per profile; that is a steady-state policy and
  complementary to this reactive guard. Not tested on this model.
- asusctl 6.5.0: `asus-armoury` exposes only `charge_mode` here (no power limits),
  and the `asus` hwmon device has no fan-curve points. No better knob.

## Development

```sh
nix develop
cargo test
nix build
```

## Tests

`tests/service.rs`: the service against a fake `/sys` tree and a scripted event
stream, one tick per second. `tests/process.rs`: the binary against a fake tree and
a log file that is appended to.

Covered: no write without an event; stepped raise; maximum clamp; settle, probe
interval, step down, release at the original offset; repeated event while throttled;
event kept across a failed step; restore on finish, on SIGTERM, and after SIGKILL via
the note; wrong product (exit 78, tree untouched); unwritable attribute (typed
error, recovers); invalid attribute content; only the one attribute changes; kernel
record matching; events from before start ignored.

### Mutation checks

Each mutation was applied by hand, the suite run, and the change reverted.

| Mutation | Result |
|---|---|
| step-up adds nothing | killed |
| no `max_offset` clamp | killed |
| no wait before step down | killed |
| no settle time | killed |
| never released to original offset | killed |
| trip not reported on event while holding | survived, then killed by a control test |
| base overwritten by repeated event | killed |
| `finish` restores nothing | killed |
| note not saved | killed |
| product check removed | killed |
| attribute `open` error swallowed | killed |
| attribute `write_all` error swallowed | survives: not reproducible on a regular file |
| kernel facility check removed | killed |
| prefix match loosened to substring | killed |
| log not opened at its end | killed |
| event dropped when a step fails | killed |
| state advances before the write succeeds | survived, then killed by an added assertion |

### Not tested

- The systemd unit. `ReadWritePaths` on a sysfs file under `ProtectKernelTunables`
  and `/dev/kmsg` under `DevicePolicy=closed` need a real system. If the first write
  fails the log shows `error: cannot write ...`; the fallback is to drop those two
  settings.
- Behaviour on hardware with the lid closed under load.
