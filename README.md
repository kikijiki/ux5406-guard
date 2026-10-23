# ux5406-guard

Power guard for the ASUS Zenbook S 14 UX5406SA (Intel Lunar Lake). It caps package
power while the lid is closed, and harder after the EC's thermal warning, so the
EC does not cut the power. It never sleeps, suspends, hibernates or powers off the
machine.

## Problem

With the lid closed and a sustained all-core load, the EC raises a warning that the
kernel logs as `asus_wmi: Unknown key code 0x6d` (ACPI query `_QB3`). About 3 minutes
after the last event it cuts the power, leaving no trace in the journal (180 to
183 s in three cuts recorded at 1 s resolution). Windows hibernates on this event;
Linux ignores it.

Observed on one unit (BIOS UX5406SA.309, kernel 7.2), always on AC:

- Lid closed, package held at the firmware limit (22 W): the event comes about
  127 s into the load, every time.
- Lid open, same load, 15 minutes each at 22 W, 16 W and 14 W: no event.
- Package temperature is not the trigger: events at 83 to 86 C, a cut at 75 C.
  Raising the TCC offset to throttle earlier did not prevent three cuts.
- Capping package power avoids it with the lid closed:

  | Cap (both RAPL limits), full load | Result |
  |---|---|
  | 9 W, applied on the event | no cut, twice |
  | 12 W | 15 min, no event |
  | 14 W | 20 min, no event |
  | 16 W | event after 127 s |

  The EC's line is somewhere between 14 and 16 W. A cap costs roughly 35 to 40
  percent of the sustained power. 15 W was not tested.

The 127 s may be a power-over-time rule in the EC; that was not established.

## Mechanism

Package power has three levels, set through both RAPL limits
(`intel-rapl:0` and `intel-rapl-mmio:0`, `constraint_0_power_limit_uw`; the lower
one wins):

- Lid open: the limits found at start, untouched.
- Lid closed: `cap` (13 W).
- After an event, lid open or closed: `safe` (9 W) for `hold` seconds (300). A
  further event extends it. Then back to the lid's level.

Reads `/dev/kmsg` (kernel records only, from the end) and
`/proc/acpi/button/lid/*/state`. The limits are checked every second and put back
if something else changes them while capped.

Only the two limit files are written. The guard refuses to run unless the DMI
product name contains `UX5406SA` (exit status 78, not restarted). The original
limits are noted in `/run/ux5406-guard` before the first change and restored on
SIGTERM/SIGINT; after a crash or SIGKILL the unit's `ExecStopPost`
(`ux5406-guard restore`) or the next start does it from the note. Read and write
failures are logged and retried; the service keeps running.

## Limits

- The first event after a long idle is not prevented, only answered. At 13 W none
  was seen.
- The 14 W figure is one 20-minute run; the default keeps a margin below it. The
  cap is not accepted above 14 W.
- The EC's actual condition is unknown. A different charger, BIOS or ambient
  temperature may move the line.
- No shutdown logic. The kernel thermal core's trips (103 and 110 C here) are the
  only backstop.

## Usage

Settings (defaults): `--cap 13` W (5..=14), `--safe 9` W (5..=cap), `--hold 300` s
(30..=3600). `ux5406-guard restore` writes back saved limits and exits.

NixOS:

```nix
inputs.ux5406-guard.url = "github:kikijiki/ux5406-guard";
# modules:
imports = [ inputs.ux5406-guard.nixosModules.default ];
services.ux5406-guard.enable = true;
# options: capWatts safeWatts holdSeconds
```

Log: `journalctl -u ux5406-guard`. Examples:

```
lid closed: package power capped at 13 W
event: asus_wmi: Unknown key code 0x6d
safe level: package power capped at 9 W for 300 s (lid closed)
lid open: original limits
stopping: limits back to 37000000 / 22000000 uW
error: cannot write /sys/...: Read-only file system
```

Remove: disable the option and rebuild. Stopping the unit restores the limits.

## Related

- thermald runs in polling mode on this model with no usable tables (empty ACPI
  `_TRT`) and does not touch the limits.
- asusctl 6.5.0: `asus-armoury` exposes only `charge_mode` here (no power limits),
  and the `asus` hwmon device has no fan-curve points.

## Development

```sh
nix develop
cargo test
nix build
```

## Tests

`tests/service.rs`: the service against a fake `/sys` and `/proc` tree and a
scripted event stream, one tick per second. `tests/process.rs`: the binary against
a fake tree and a log file that is appended to.

Covered: no write with the lid open; cap on close and restore on open; safe level
for the hold, extension by a second event, event with the lid open; limits changed
by someone else put back; the original is the first value found; event kept across
a failed step; restore on finish, on SIGTERM, and after SIGKILL via the note; wrong
product (exit 78, tree untouched); missing lid; unwritable limit (typed error,
recovers, original survives); invalid limit content; only the two limits change;
kernel record matching; events from before start ignored; setting ranges.

### Mutation checks

Each mutation was applied by hand, the suite run, and the change reverted.

| Mutation | Result |
|---|---|
| cap applied with the lid open instead of closed | detected: the suite hangs instead of failing |
| hold ends one second late | killed |
| second event does not extend the hold | killed |
| note not saved | killed |
| limits changed by someone else not put back | killed |
| original overwritten by a capped value | killed |
| product check removed | killed |
| firmware-limit write error swallowed | survived, then killed by an added test |
| `finish` restores nothing | killed |

### Not tested

- The systemd unit. `ReadWritePaths` on sysfs files under `ProtectKernelTunables`,
  `/proc/acpi` under `ProcSubset=all` and `/dev/kmsg` under `DevicePolicy=closed`
  need a real system.
- Behaviour on hardware with the lid closed under load. The measurements above were
  made with hand-written scripts that set the same limits.
