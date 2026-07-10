//! lg-brightness — control the backlight brightness of LG UltraFine monitors
//! (and, as a bonus, Apple Studio Display / Pro Display XDR) from Linux.
//!
//! These monitors have no physical buttons. On macOS, brightness is set over
//! a USB HID "Monitor" interface rather than the usual DDC/CI-over-DisplayPort
//! mechanism, which is why generic tools like `ddcutil` don't work with them.
//! This tool talks that USB HID protocol directly via libusb.
//!
//! Protocol reference (independently verified against ei-grad/usb-hid-brightness,
//! MIT licensed): the monitor exposes several USB HID interfaces; the one whose
//! iInterface string descriptor equals "HID BRIGHTNESS" accepts a 6-byte Feature
//! report, report ID 0, holding a little-endian u16 brightness value in bytes
//! 0-1, over the range 0-54000.

use std::process::exit;
use std::time::Duration;

use rusb::{Context, Device, DeviceHandle, UsbContext};

/// LG's USB vendor ID (also used by these Apple-Thunderbolt-Display-derived monitors).
const LG_VENDOR_ID: u16 = 0x043e;

const LG_REPORT_ID: u8 = 0;
const LG_REPORT_LEN: usize = 6;
const LG_VALUE_OFFSET: usize = 0;
const LG_VALUE_SIZE: usize = 2;
const LG_MIN: u32 = 0;
const LG_MAX: u32 = 54000;

const TIMEOUT: Duration = Duration::from_millis(1000);

const HID_GET_REPORT: u8 = 0x01;
const HID_SET_REPORT: u8 = 0x09;
const HID_REPORT_TYPE_FEATURE: u16 = 0x0300;

// bmRequestType bytes, matching the USB HID class control-transfer direction.
const REQTYPE_IN_CLASS_INTERFACE: u8 = 0b1010_0001;
const REQTYPE_OUT_CLASS_INTERFACE: u8 = 0b0010_0001;

struct BrightnessDevice {
    handle: DeviceHandle<Context>,
    interface: u8,
    product: String,
}

fn find_brightness_devices(ctx: &Context) -> rusb::Result<Vec<BrightnessDevice>> {
    let mut found = Vec::new();

    for device in ctx.devices()?.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if desc.vendor_id() != LG_VENDOR_ID {
            continue;
        }

        let handle = match device.open() {
            Ok(h) => h,
            Err(_) => continue, // e.g. permission denied; see udev rule in README
        };

        if let Some(iface) = find_hid_brightness_interface(&device, &handle, &desc) {
            let product = handle
                .read_string_descriptor_ascii(desc.product_string_index().unwrap_or(0))
                .unwrap_or_else(|_| "LG UltraFine Display".to_string());
            found.push(BrightnessDevice {
                handle,
                interface: iface,
                product,
            });
        }
    }

    Ok(found)
}

/// Scan every interface's iInterface string for the literal "HID BRIGHTNESS"
/// marker that the monitor firmware uses to identify the correct interface.
fn find_hid_brightness_interface(
    device: &Device<Context>,
    handle: &DeviceHandle<Context>,
    desc: &rusb::DeviceDescriptor,
) -> Option<u8> {
    for cfg_idx in 0..desc.num_configurations() {
        let config = match device.config_descriptor(cfg_idx) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for interface in config.interfaces() {
            for idesc in interface.descriptors() {
                let str_idx = match idesc.description_string_index() {
                    Some(i) if i != 0 => i,
                    _ => continue,
                };
                if let Ok(s) = handle.read_string_descriptor_ascii(str_idx) {
                    if s == "HID BRIGHTNESS" {
                        return Some(idesc.interface_number());
                    }
                }
            }
        }
    }
    None
}

fn with_interface_claimed<T>(
    dev: &mut BrightnessDevice,
    f: impl FnOnce(&mut DeviceHandle<Context>, u8) -> rusb::Result<T>,
) -> rusb::Result<T> {
    let iface = dev.interface;
    let had_kernel_driver = dev.handle.kernel_driver_active(iface).unwrap_or(false);
    if had_kernel_driver {
        dev.handle.detach_kernel_driver(iface)?;
    }

    dev.handle.claim_interface(iface)?;
    let result = f(&mut dev.handle, iface);
    let _ = dev.handle.release_interface(iface);

    if had_kernel_driver {
        let _ = dev.handle.attach_kernel_driver(iface);
    }

    result
}

fn get_brightness(dev: &mut BrightnessDevice) -> rusb::Result<u32> {
    with_interface_claimed(dev, |handle, iface| {
        let mut buf = [0u8; LG_REPORT_LEN];
        handle.read_control(
            REQTYPE_IN_CLASS_INTERFACE,
            HID_GET_REPORT,
            HID_REPORT_TYPE_FEATURE | LG_REPORT_ID as u16,
            iface as u16,
            &mut buf,
            TIMEOUT,
        )?;

        let mut val: u32 = 0;
        for i in 0..LG_VALUE_SIZE {
            val |= (buf[LG_VALUE_OFFSET + i] as u32) << (8 * i);
        }
        Ok(val)
    })
}

fn set_brightness(dev: &mut BrightnessDevice, value: u32) -> rusb::Result<()> {
    with_interface_claimed(dev, |handle, iface| {
        let mut buf = [0u8; LG_REPORT_LEN];
        buf[0] = LG_REPORT_ID;
        for i in 0..LG_VALUE_SIZE {
            buf[LG_VALUE_OFFSET + i] = ((value >> (8 * i)) & 0xff) as u8;
        }

        handle.write_control(
            REQTYPE_OUT_CLASS_INTERFACE,
            HID_SET_REPORT,
            HID_REPORT_TYPE_FEATURE | LG_REPORT_ID as u16,
            iface as u16,
            &buf,
            TIMEOUT,
        )?;
        Ok(())
    })
}

fn raw_to_percent(raw: u32) -> u32 {
    ((raw.saturating_sub(LG_MIN)) as u64 * 100 / (LG_MAX - LG_MIN) as u64) as u32
}

fn percent_to_raw(pct: u32) -> u32 {
    let pct = pct.min(100);
    LG_MIN + (pct as u64 * (LG_MAX - LG_MIN) as u64 / 100) as u32
}

fn usage() -> ! {
    eprintln!(
        "lg-brightness — control LG UltraFine monitor brightness over USB HID\n\n\
         Usage:\n  \
         lg-brightness get                 Show current brightness (0-100%)\n  \
         lg-brightness set <0-100>         Set brightness to a percentage\n  \
         lg-brightness inc <N>             Increase brightness by N percent\n  \
         lg-brightness dec <N>             Decrease brightness by N percent\n  \
         lg-brightness get --raw           Show raw value (0-{max})\n  \
         lg-brightness set --raw <0-{max}> Set raw value directly\n",
        max = LG_MAX
    );
    exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }

    let ctx = Context::new().unwrap_or_else(|e| {
        eprintln!("Failed to initialize libusb: {e}");
        exit(1);
    });

    let mut devices = find_brightness_devices(&ctx).unwrap_or_else(|e| {
        eprintln!("Failed to enumerate USB devices: {e}");
        exit(1);
    });

    if devices.is_empty() {
        eprintln!(
            "No LG UltraFine brightness-control interface found.\n\
             - Check the monitor is connected via USB-C/Thunderbolt (not just video).\n\
             - Check permissions: run with sudo, or install a udev rule (see README)."
        );
        exit(1);
    }

    let raw_mode = args.iter().any(|a| a == "--raw");
    let positional: Vec<&String> = args.iter().skip(1).filter(|a| *a != "--raw").collect();

    match positional.first().map(|s| s.as_str()) {
        Some("get") => {
            for dev in devices.iter_mut() {
                match get_brightness(dev) {
                    Ok(raw) => {
                        if raw_mode {
                            println!("{}: {}", dev.product, raw);
                        } else {
                            println!("{}: {}%", dev.product, raw_to_percent(raw));
                        }
                    }
                    Err(e) => eprintln!("{}: failed to read brightness: {e}", dev.product),
                }
            }
        }
        Some("set") => {
            let Some(val_str) = positional.get(1) else {
                usage();
            };
            let Ok(val) = val_str.parse::<u32>() else {
                usage();
            };
            let raw = if raw_mode { val.min(LG_MAX) } else { percent_to_raw(val) };
            for dev in devices.iter_mut() {
                match set_brightness(dev, raw) {
                    Ok(()) => println!(
                        "{}: set to {}",
                        dev.product,
                        if raw_mode {
                            raw.to_string()
                        } else {
                            format!("{val}%")
                        }
                    ),
                    Err(e) => eprintln!("{}: failed to set brightness: {e}", dev.product),
                }
            }
        }
        Some("inc") | Some("dec") => {
            let sign: i64 = if positional[0] == "inc" { 1 } else { -1 };
            let Some(val_str) = positional.get(1) else {
                usage();
            };
            let Ok(delta) = val_str.parse::<i64>() else {
                usage();
            };
            for dev in devices.iter_mut() {
                let current = match get_brightness(dev) {
                    Ok(raw) => raw,
                    Err(e) => {
                        eprintln!("{}: failed to read brightness: {e}", dev.product);
                        continue;
                    }
                };
                let current_pct = raw_to_percent(current) as i64;
                let new_pct = (current_pct + sign * delta).clamp(0, 100) as u32;
                let new_raw = percent_to_raw(new_pct);
                match set_brightness(dev, new_raw) {
                    Ok(()) => println!("{}: {}% -> {}%", dev.product, current_pct, new_pct),
                    Err(e) => eprintln!("{}: failed to set brightness: {e}", dev.product),
                }
            }
        }
        _ => usage(),
    }
}
