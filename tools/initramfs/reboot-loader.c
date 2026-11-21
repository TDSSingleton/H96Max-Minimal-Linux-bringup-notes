/*
 * reboot-loader: reboot(2) with LINUX_REBOOT_CMD_RESTART2 and the string "loader".
 * busybox `reboot` cannot pass a command string. The vendor kernel's syscon-reboot-mode
 * then writes mode-loader 0x5242c301 to 0xff370200, and our u-boot's boot_mode patch
 * (tools/uboot/0001-*.patch) turns that into BootROM download mode = maskrom.
 * A plain `reboot -f` would write mode-normal there instead, so it cannot be used.
 *
 * No libc (the cross toolchain has no aarch64 sysroot): raw aarch64 syscalls.
 * Build: aarch64-linux-gnu-gcc -Os -static -nostdlib -ffreestanding -fno-stack-protector
 */
#define NR_sync   81
#define NR_exit   93
#define NR_reboot 142

static long sc4(long n, long a, long b, long c, long d)
{
	register long x8 __asm__("x8") = n;
	register long x0 __asm__("x0") = a;
	register long x1 __asm__("x1") = b;
	register long x2 __asm__("x2") = c;
	register long x3 __asm__("x3") = d;
	__asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3) : "memory");
	return x0;
}

void _start(void)
{
	static const char cmd[] = "loader";
	sc4(NR_sync, 0, 0, 0, 0);
	/* LINUX_REBOOT_MAGIC1, MAGIC2, LINUX_REBOOT_CMD_RESTART2 */
	sc4(NR_reboot, 0xfee1dead, 672274793, 0xA1B2C3D4, (long)cmd);
	sc4(NR_exit, 1, 0, 0, 0);   /* only reached if reboot failed */
	for (;;) ;
}
