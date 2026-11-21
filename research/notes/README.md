# H96Max / RK3518 — research notes (index)

Split from the single `NOTES.md` (2026-11-21) so each file holds at most two subjects.
Text is unchanged except where marked; `§N` numbers are the original ones.

Current state (2026-11-21 evening): hardware layer done -- our u-boot + arm64 kernel + Alpine on the box, WiFi, Bluetooth remote, console UI. Next: hardware video (§33).

| § | Title | File |
|---|---|---|
| 1 | GammaOS does not support this device — VERIFIED MYSELF | [01-goal-and-firmware-landscape.md](01-goal-and-firmware-landscape.md) |
| 2 | RK3518 is an RK3528-family part, not its own chip | [02-soc-identity.md](02-soc-identity.md) |
| 3 | Storage / boot / flash | [03-storage-and-usb.md](03-storage-and-usb.md) |
| 4 | USB — why the single port hurts | [03-storage-and-usb.md](03-storage-and-usb.md) |
| 5 | Root-by-ramdisk — the premise needs updating | [04-security-and-root.md](04-security-and-root.md) |
| 6 | Security posture — separating fact from folklore | [04-security-and-root.md](04-security-and-root.md) |
| 7 | Firmware availability — the real blocker | [01-goal-and-firmware-landscape.md](01-goal-and-firmware-landscape.md) |
| 8 | Local workstation state (Fedora 45, x86_64) | [05-workstation-and-loaders.md](05-workstation-and-loaders.md) |
| 9 | Open questions to resolve before any flashing | [06-device-readout.md](06-device-readout.md) |
| 10 | Second pass — tooling built, and the Hugsun X88PRO as our Rosetta stone | [05-workstation-and-loaders.md](05-workstation-and-loaders.md) |
| 11 | RK3518 vs RK3528 — resolved from Rockchip's own repo, not forum writeups | [02-soc-identity.md](02-soc-identity.md) |
| 12 | First read-out of the real device — and a 32 MiB read ceiling that invalidated most of it | [06-device-readout.md](06-device-readout.md) |
| 13 | Linux on RK3528/RK3518 — kernel choice, what works, what does not exist | [08-kernel.md](08-kernel.md) |
| 14 | Verified device facts, the read ceiling solved, and where the source lives | [06-device-readout.md](06-device-readout.md) |
| 15 | u-boot, the build environment, and the community FIT we rejected | [09-bootloader-and-boot-image.md](09-bootloader-and-boot-image.md) |
| 16 | The bootloader question dissolved: the stock u-boot already boots our kernel | [09-bootloader-and-boot-image.md](09-bootloader-and-boot-image.md) |
| 17 | Will there be picture on HDMI? — answered from the extracted firmware, not hope | [10-hdmi-and-console.md](10-hdmi-and-console.md) |
| 18 | Corrections (2026-11-21) — re-checked against the bytes on disk | [09-bootloader-and-boot-image.md](09-bootloader-and-boot-image.md) |
| 19 | What the dump says beyond the boot chain (2026-11-21) | [07-dump-contents.md](07-dump-contents.md) |
| 20 | The factory kernel's own `.config` (2026-11-21) | [08-kernel.md](08-kernel.md) |
| 21 | What HDMI will show, and the first-boot console fixes (2026-11-21) | [10-hdmi-and-console.md](10-hdmi-and-console.md) |
| 22 | arm64 decided — and the stock u-boot cannot start an arm64 kernel (2026-11-21) | [09-bootloader-and-boot-image.md](09-bootloader-and-boot-image.md) |
| 23 | Our 64-bit u-boot, built and its boot script executed (2026-11-21) | [09-bootloader-and-boot-image.md](09-bootloader-and-boot-image.md) |
| 24 | WiFi: SWT6621S driver port from Neardi's SDK | [11-wifi.md](11-wifi.md) |
| 25 | USB debug link on the USB-C port (built, then removed: brown-out) | [11-wifi.md](11-wifi.md) |
| 26 | WiFi works: chip boot, wlan0, scan on 2.4 + 5 GHz | [11-wifi.md](11-wifi.md) |
| 27 | WPA2 connects: the SWT6621S rx descriptor, decoded from the ODM driver | [11-wifi.md](11-wifi.md) |
| 28 | WiFi at boot (h96-wifi), and the no-RTC clock skew that hid it | [11-wifi.md](11-wifi.md) |
| 29 | h96ui: the console interface | [12-interface-and-video.md](12-interface-and-video.md) |
| 30 | Video: software playback not viable; VPU + VOP2-plane plan | [12-interface-and-video.md](12-interface-and-video.md) |
| 31 | Input: no IR receiver (stick), CEC unavailable; gpio-watch method | [13-input-and-bluetooth.md](13-input-and-bluetooth.md) |
| 32 | Bluetooth: skwbt port, lite NV, codec-query crash, remote paired | [13-input-and-bluetooth.md](13-input-and-bluetooth.md) |
| 33 | Starting point for the VPU / GPU job (MPP, RGA, VOP2 planes, Mali-450) | [12-interface-and-video.md](12-interface-and-video.md) |

Related: `../vendor-kernel-6.1.118.config` (factory kernel `.config`, §20).
