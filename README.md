# linux-legion

[![Vibe-coded with Claude Code](https://img.shields.io/badge/vibe--coded-Claude%20Code-d97757)](#how-it-is-built)

A Linux control center for **Lenovo Legion** laptops — what Lenovo Vantage / Legion Space does on
Windows: power modes, CPU power limits, fans, per-key **Spectrum RGB** lighting, battery charging
modes and hardware switches.

Written in Rust on [syngui](https://github.com/VitaminDB/syngui). Uses the upstream kernel drivers
(`lenovo-wmi-gamezone`, `lenovo-wmi-other`, `ideapad-laptop`) and talks to the RGB controller
directly through `hidraw` — no out-of-tree kernel module, no root at runtime (a small user
service keeps the CPU power limits and fans in line with the selected mode).

<p align="center"><img src="assets/icon/linux-legion-128.png" width="96" alt="icon"></p>

![Home](screenshots/home.png)

| Spectrum lighting — live map | Effect layers |
|---|---|
| ![Lighting](screenshots/lighting.png) | ![Layers](screenshots/lighting-layers.png) |

| Performance | Battery | Device |
|---|---|---|
| ![Performance](screenshots/performance.png) | ![Battery](screenshots/battery.png) | ![Device](screenshots/device.png) |

## What it does

- **Home** — power mode in one click, live CPU / GPU / memory / battery gauges and fan speeds.
  The NVIDIA GPU is polled only while it is already awake, so the dashboard never wakes the dGPU.
  While it is awake the GPU card lists the apps keeping it awake, and **Deactivate GPU** asks your
  own apps among them to close (compositors and system processes are never touched).
- **Performance** — Quiet / Balance / Performance / Extreme / Custom (the same as Fn+Q).
  In *Custom*: CPU PL1/PL2 and every other limit the firmware exposes (with a *Maximum* preset),
  and manual fan targets.
  **CPU limits per mode (RAPL)**: on the Legion Pro 7 Gen 10 the BIOS writes 30/30 W into RAPL
  MMIO at boot and never touches it again — on Windows Legion Space applies the per-mode limits,
  on Linux nobody does, so the CPU is stuck at 30 W in every mode. The service writes Lenovo's own
  values (Quiet 55/65, Balance 90/125, Performance 145/190, Extreme 160/205 W, Custom = the
  firmware limits you set) into both RAPL interfaces and keeps them there.
  **CPU Boost**: turbo on or off for every mode (`intel_pstate/no_turbo`, or `cpufreq/boost` on
  other drivers); the service restores your choice after a reboot.
  **EPP per mode** (optional): `performance` in Performance/Extreme/Custom, `balance_performance`
  in Balance, `balance_power` in Quiet — overrides tuned / power-profiles-daemon.
  **Custom Mode presets**: save the current limits and fan speeds under a name and switch between
  them (also from actions and the command line); the active preset is what the service restores.
  **Maximum fan speed**: every fan at its rated maximum in any mode — it switches the software
  curve on too, so the fans can come back afterwards.
  **Software fan curve**: see [Fans](#fans) below.
  **Auto-apply**: the firmware forgets the Custom values after a reboot (fan targets also after
  sleep); the service restores them at login, whenever Custom mode is switched on (Fn+Q) and after
  resume.
- **Lighting (Spectrum)** — six hardware profiles (Fn+Space), brightness, the LEGION lid logo,
  and a layer editor: select keys and case zones on a live map (colours are read back from the
  controller ~10 times a second) and assign any of 12 effects with speed, direction and colours.
  Profiles are stored in the keyboard itself. **Profile files** use Legion Toolkit's JSON format,
  so a profile exported on Windows imports here and the other way round. ANSI, ISO and JIS
  keyboards are drawn with their own layout (detected from the controller's key codes, as Legion
  Toolkit does).
- **Battery** — battery mode (Normal / Rapid Charge / Conservation ≈80 %), charge and health,
  the power source (AC adapter or USB-C with its wattage, with a *low wattage charger* warning),
  how long it has been on battery and the lowest/highest discharge rate since unplugging.
- **Device** — Fn Lock, camera kill switch, always-on USB charging, system information, and which
  desktop notifications the service shows (power mode incl. Fn+Q, Fn Lock, camera, AC adapter,
  actions).
- **Actions** — Legion Toolkit's automation: *when* something happens, run a list of steps. See
  [Actions](#actions).
- **Tray icon** — power mode, Custom Mode presets, CPU Boost, maximum fan speed, battery mode,
  keyboard profiles and quick actions in one menu; a click opens the window (only one window ever
  runs — launching again brings it forward). The service shows it from login, so it is there with
  the window closed. In GNOME it needs the AppIndicator extension (`ubuntu-appindicators` /
  `appindicatorsupport`); it can be turned off on the Device page.

## Supported hardware

Developed and tested on **Legion Pro 7 16IAX10H** (83F5, Intel Core Ultra 9 275HX,
RTX 5090 Laptop, Spectrum controller ITE `048d:c197`), Arch Linux, kernel 7.0.

Other Legion models with the same kernel interfaces and a Spectrum controller (`048d:c1xx` /
`048d:c9xx`, feature report `0x07` of 960 bytes) should work; every page adapts to what the
firmware reports. `linux_legion --simulate` shows the whole UI without the hardware.

## Install

### Arch Linux (AUR)

```bash
yay -S linux-legion        # or paru -S linux-legion
```

The package installs `/usr/bin/linux-legion`, a menu entry, icons and the udev rule.

### From source

```bash
git clone https://github.com/VitaminDB/linux-legion
cd linux-legion
cargo build --release
sudo install -Dm644 packaging/70-linux-legion.rules /etc/udev/rules.d/70-linux-legion.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
./target/release/linux_legion
```

The udev rule gives the logged-in user access to the RGB controller (`uaccess`) and makes the
sysfs knobs (power mode, power limits, fan targets, RAPL, EPP, battery mode, Fn Lock…) writable
for the `wheel` group — or for `sudo` on distributions without `wheel` (Debian, Ubuntu, PikaOS).
Your user has to be in that group.

## Usage

```bash
linux_legion                 # control the laptop
linux_legion --apply         # apply saved Custom-mode limits and fans once
linux_legion --daemon        # the auto-apply service (linux-legion-autoapply.service)
linux_legion --simulate      # demo mode with a simulated Legion Pro 7
linux_legion --page 2        # open a page (0 home … 5 actions)
LEGION_TRACE=1 linux_legion  # print every RGB packet to stderr
```

### Command line

Like Legion Toolkit's `llt`, every feature can be read and changed without the window — handy for
desktop keyboard shortcuts and scripts (add `--simulate` to try it without the hardware):

```bash
linux_legion list                           # every feature and its current value
linux_legion get power-mode                 # → balanced
linux_legion set power-mode performance
linux_legion set preset Gaming              # Custom Mode preset
linux_legion set cycle-preset               # next preset
linux_legion set fan-max on
linux_legion set spectrum-profile 3
linux_legion values battery-mode            # values a feature accepts
linux_legion run "Gaming"                   # run a quick action
linux_legion actions                        # list the actions
linux_legion spectrum info                  # controller, zones, keyboard layout
linux_legion spectrum export 3 wave.json    # lighting profile → Legion Toolkit file
linux_legion spectrum import 3 wave.json
```

Features: `power-mode`, `battery-mode`, `preset`, `cycle-preset`, `fan-max`, `cpu-boost`, `fn-lock`, `camera`,
`always-on-usb`, `spectrum-profile`, `spectrum-brightness`, `panel-logo`.

## The service

Everything the service does is configured in `~/.config/linux-legion/custom.conf` (the toggles
on the Performance page edit it); the toggle *Service* runs
`systemctl --user enable --now linux-legion-autoapply.service`.

```text
tunable.ppt_pl1_spl=95       # Custom mode: firmware limits, applied with "Apply"
fan.1=3000                   # Custom mode: fan target, 0 = automatic
fan.curve=1                  # software fan curve (see Fans)
cpu.rapl=1                   # per-mode CPU limits in RAPL (default on)
cpu.epp=0                    # per-mode EPP (default off)
rapl.performance=150/190     # your own PL1/PL2 for a mode (quiet, balanced, performance, extreme)
fan.max=0                    # Maximum fan speed
cpu.boost=1                  # CPU Boost (turbo); no line = leave it alone
tray=1                       # tray icon
notify.camera=0              # turn one notification off (power_mode, fn_lock, camera, ac_adapter, automation)
```

Custom Mode presets live in `presets.conf` next to it, actions in `automation.conf`.

`linux_legion --apply` applies everything once; `--daemon` is what the unit runs. The service
re-checks the limits every 5 s (writes only when something differs), the fans every 2 s, and
watches for the triggers of your actions every second. Notifications go through `notify-send`
(or `gdbus` when it is missing).

## Actions

An action is a trigger and a list of steps, run in order (the first failing step stops the
rest), like Legion Toolkit's Actions page. Actions with a trigger run in the service; *quick
actions* (the manual trigger) run from the Actions page or with `linux_legion run NAME`. The page
edits `~/.config/linux-legion/automation.conf`, which can also be written by hand:

```text
[On battery]
enabled=1
trigger=ac-disconnected
step=power-mode quiet
step=spectrum-brightness 2

[Gaming]
trigger=processes-started steam, cs2, game.exe
step=preset Gaming
step=spectrum-profile 4
```

| Triggers | Steps |
|---|---|
| `manual` — quick action | `power-mode quiet\|balanced\|performance\|extreme\|custom` |
| `ac-connected`, `ac-disconnected` | `battery-mode normal\|rapid-charge\|conservation` |
| `low-wattage-charger` — powered over USB-C only | `preset NAME`, `cycle-preset` |
| `battery-below 20` — on battery, crossing 20 % | `fan-max on\|off`, `cpu-boost on\|off` |
| `power-mode extreme` — also when changed with Fn+Q | `fn-lock`, `camera`, `always-on-usb` `on\|off` |
| `processes-started a, b`, `processes-stopped a, b` | `spectrum-profile 1…6`, `spectrum-brightness 0…9` |
| `lid-opened`, `lid-closed` | `panel-logo on\|off`, `spectrum-import FILE.json` |
| `display-connected`, `display-disconnected` — external | `notify TEXT`, `run COMMAND`, `delay SECONDS` |
| `time 22:30` — every day, `startup` — at login, `resume` | |

Triggers fire on the change, not while the condition holds. App names match the process name or
the executable (`game` also matches `game.exe` under Wine/Proton). A rule with an error in
`automation.conf` is reported in the service log (`journalctl --user -u linux-legion-autoapply`)
and disabled.

## Fans

The kernel's `fanN_target` (0 = auto) talks to the EC command that Legion Space uses for its
per-fan RPM sliders. On the Legion Pro 7 Gen 10 EC (83F5 / Q7CN) that command is **one-way**: once
a manual RPM has been written, the EC never goes back to its own curve — not after writing 0
(that stops the fan until the thermal floor kicks in), not after a mode switch, the fan table,
the full-speed flag, suspend or a warm reboot. Only an EC reset helps: shut down, unplug the
charger, hold the power button for 30 s (or the pin-hole reset), boot.

So *Software fan curve* exists: when it is on, the service drives all fans by CPU / GPU
temperature (fan 1 — CPU, fan 2 — GPU, fan 4 — PCH, driven by the hotter of the two) with a curve scaled by the
mode (Quiet ×0.7 … Extreme ×1.35); in Custom mode the targets you set are kept as they are, except
above 95 °C where they are raised to the curve. Speeds rise immediately and fall only after three
consecutive lower readings, so they do not hunt. It turns on automatically the first time you
apply a manual fan target.

## Kernel interfaces

| Feature | Interface |
|---|---|
| Power mode | `/sys/class/platform-profile/*/profile` (`lenovo-wmi-gamezone`) |
| Power limits | `/sys/class/firmware-attributes/lenovo-wmi-other-0/attributes/*/current_value` — accepted only in *custom* mode, otherwise `EBUSY` |
| Fans | hwmon `lenovo_wmi_other`: `fanN_input`, `fanN_target` (0 = auto on paper; see [Fans](#fans)) |
| CPU limits | powercap `intel-rapl:0` and `intel-rapl-mmio:0`, `constraint_{0,1}_power_limit_uw` |
| EPP | cpufreq `policy*/energy_performance_preference` |
| Battery mode | `/sys/class/power_supply/BAT0/charge_types` (`Fast` / `Standard` / `Long_Life`) |
| Switches | `ideapad_acpi`: `fn_lock`, `camera_power`, `usb_charging` |

## Spectrum protocol

For anyone porting this elsewhere. The command set matches the one Lenovo Legion Toolkit
reverse-engineered for earlier generations; the Gen 10 findings below were made on real hardware.

- One **feature report `0x07`, 960 bytes**. Request `[07] [op] [C0] [03] [params…]` via
  `HIDIOCSFEATURE`, the answer is read with `HIDIOCGFEATURE` as `[07] [op] [len] [00] [data…]`.
- `HIDIOCGFEATURE` without a pending answer returns the **current frame**: `[07] [03] …` followed by
  `(u16 zone, r, g, b)` records — colours already scaled by brightness.
- Operations: `D1` compatibility, `C4`/`C5` zone matrix (22 × 9 on the Pro 7, plus the lid logo
  `0x5DD`), `CA`/`C8` get/set profile, `CD`/`CE` brightness 0–9, `A5`/`A6` logo, `CC`/`CB`
  get/set profile layers, `C9` factory profile.
- Layer record: `[n] 06 01 [type] 02 [speed] 03 [rotation] 04 [direction] 05 [colour mode] 06 00
  [#colours] [rgb…] [#zones] [u16 zone…]`. Full encoder/decoder with tests:
  [`src/spectrum/protocol.rs`](src/spectrum/protocol.rs).
- **Gen 10 quirks** (`048d:c197`):
  - the answer takes up to ~50 ms; reading too early returns the *previous* answer and drops the
    pending one, so the request has to be re-sent;
  - an unread answer stays in the buffer until the next read — even across processes — so the
    buffer is flushed before every request;
  - **reading a profile's layers (`CC`) makes that profile active**;
  - the length byte of `CB` is echoed back on read; the firmware's own profiles use `C0`.
- Case zones on the Pro 7 16IAX10H: rear vents `0x3E9–0x3FA` (18), sides `0x1F5/0x1F6/0x1FD/0x1FE`,
  front light bar `0x1F7–0x1FC`, lid logo `0x5DD`. The second ITE device (`048d:c193`, "Lenovo
  Lighting") speaks a different protocol and is not used yet.

## Project layout

```text
src/hw/         sysfs: power modes and limits, fans, battery, charger, switches, sensors, lid/displays/
                apps for triggers, apps holding the dGPU, demo sysfs tree
src/spectrum/   RGB protocol, hidraw transport, simulator, ANSI/ISO/JIS layouts, Legion Toolkit profile files
src/worker.rs   background thread: polling and hardware commands
src/ui/         the window: home, performance, lighting, battery, device and actions pages
src/autoapply.rs  the service: Custom-mode auto-apply, per-mode RAPL/EPP, software fan curve,
                  notifications, running actions
src/actions.rs    action steps, shared by actions, quick actions and the command line
src/automation.rs action triggers, automation.conf, when a trigger fires
src/presets.rs    Custom Mode presets (presets.conf)
src/cli.rs        the command line
src/notify.rs     desktop notifications
src/json.rs       a minimal JSON reader/writer for Legion Toolkit profile files
packaging/      udev rule, systemd user unit, .desktop entry, PKGBUILD
```

`cargo test` runs the unit tests (including packets captured from a live keyboard).
`cargo test -- --ignored live_` writes test layers to an inactive profile of the real keyboard,
reads them back and restores the original profile byte for byte.

## Compared with Lenovo Legion Toolkit

linux-legion is a Linux take on [Lenovo Legion Toolkit](https://github.com/BartoszCichecki/LenovoLegionToolkit).
What it has, and what Linux does not let it have yet:

| Legion Toolkit | linux-legion |
|---|---|
| Power modes, Custom Mode limits, presets | yes — plus per-mode CPU limits in RAPL, which Linux otherwise never sets |
| Fan curve, maximum fan speed | software fan curve driven by the service; maximum fan speed |
| Spectrum per-key lighting, profiles, brightness, logo | yes, same protocol; profile files are interchangeable |
| Aurora Sync (screen → keyboard) | no — needs screen capture through the Wayland portal |
| Battery modes, battery details | yes; no battery temperature (not exposed by the driver) |
| Fn Lock, camera, Always on USB | yes |
| Touchpad lock, Win key lock, Flip to Start, Instant Boot, Over Drive, GPU working mode (MUX) | no kernel interface on Linux yet |
| Refresh rate, resolution, HDR, DPI | left to the desktop (KDE/GNOME settings) |
| Actions / automation, quick actions | yes, see [Actions](#actions) — no Wi-Fi, session-lock or idle triggers |
| Notifications | power mode, Fn Lock, camera, AC adapter, actions |
| Deactivate GPU | yes (your own apps only) |
| CLI (`llt`) | `linux_legion get/set/run/…` |
| Tray icon with quick actions | yes — shown by the service (StatusNotifierItem) |

## Naming

The English interface uses the same wording as [Lenovo Legion
Toolkit](https://github.com/BartoszCichecki/LenovoLegionToolkit) on Windows — power modes, battery
modes, the Custom-mode power limits and every Spectrum effect, direction and speed — so the two
tools can be read side by side. The power-limit labels and descriptions come from its *Custom Mode*
window; the attribute names and units behind them were checked against the `display_name` the
`lenovo-wmi-other` driver reports on a Legion Pro 7 16IAX10H.

## How it is built

This project is vibe-coded. Since spring 2026 I write all of my projects with [Claude
Code](https://claude.com/claude-code): I decide what to build and how it fits together,
describe each task, and review, run and measure the result on my own hardware — the model
writes the code, the tests and most of the documentation. Every packet format was captured from
and tested on my own Legion laptop.

## Disclaimer

Unofficial project, not affiliated with or endorsed by Lenovo. "Lenovo", "Legion" and "Vantage"
are trademarks of their owners. Power limits and fan targets are applied through the vendor's own
firmware interfaces, but you use this tool at your own risk.

## License

[MIT](LICENSE)
