# lg-brightness

A small CLI to control the backlight brightness of the LG UltraFine 24MD4KL
(and related UltraFine/Apple display models) on Linux, including Raspberry Pi OS.

## How to use on Raspberry Pi

- Disconnect display cable
- Connect a data cable USB-C (Thunderbolt port on monitor) to USB-A (Pi end)
- SSH into computer, run `lg-brightness set 50%` for 50% brightness
- Now reconnect display cable, and disconnect data cable
- You should see the monitor in the newly set brightness

Whichever cable is connected (to one of the Thunderbolt ports) first takes over as the display cable, and the second Thunderbolt port becomes a daisy chain port. This is why display brightness cannot be seen in real time, which is a bummer. (I tried all these permutations.)

The only current benefit this has is that it now does not require a Mac to set display brightness.

## How to use on a Linux computer with USB-C / Thunderbolt port

For a computer running Linux that has a Thunderbolt port (for display), the tool "should" work as intended. ("should" here is an operative word, since I have not tested it, since I do not have a linux computer with a Thunderbolt display port.)

## Why this exists

The 24MD4KL has no physical buttons and doesn't support standard DDC/CI
brightness control over its video connection (so `ddcutil` won't work). On
macOS, brightness is instead set through a dedicated USB HID interface. This
tool talks to that same interface directly over USB via `libusb`.

## Build

On Raspberry Pi OS (Debian-based):

```bash
sudo apt update
sudo apt install -y cargo rustc libusb-1.0-0-dev pkg-config
cargo build --release
```

The binary will be at `target/release/lg-brightness`.

## Permissions

By default only root can open the monitor's USB device. Either run with
`sudo`, or install the included udev rule once so your normal user account
can control it:

```bash
sudo cp 99-lg-ultrafine-brightness.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Unplug/replug the monitor's USB cable (or reboot) after installing the rule.

## Usage

```bash
lg-brightness get              # show current brightness, 0-100%
lg-brightness set 60           # set brightness to 60%
lg-brightness inc 10           # increase brightness by 10 percentage points
lg-brightness dec 10           # decrease brightness by 10 percentage points

lg-brightness get --raw        # show the raw device value (0-54000)
lg-brightness set --raw 27000  # set the raw device value directly
```

Tip: bind `inc`/`dec` to keyboard shortcuts in Raspberry Pi OS (Preferences ->
Keyboard shortcuts, or a custom `.desktop`/`xbindkeys` entry) to get physical
brightness keys.

## How it works

The monitor exposes several USB HID interfaces. The one used for brightness
identifies itself via a `"HID BRIGHTNESS"` interface string. Brightness is
read/written as a Feature report (report ID 0, 6 bytes total, value as a
little-endian `u16` in bytes 0-1), over the range 0-54000, using standard USB
HID `GET_REPORT`/`SET_REPORT` control transfers. This tool auto-detects that
interface by vendor ID (`0x043e`, LG) and by scanning for the identifying
string, so it should also work unmodified on the 27MD5KL/27MD5KA UltraFine 5K
models.
