/*
 * gpio-watch: find which SoC pin an IR receiver (or anything else) toggles.
 * Maps the input-level registers (EXT_PORT, offset 0x70) of the five RK3528 GPIO banks via
 * /dev/mem and samples them in a tight loop for N seconds, counting level transitions per
 * pin. Prints every pin that changed, with its count and final level.
 * No libc (the cross toolchain has no aarch64 sysroot): raw aarch64 syscalls.
 *   usage: gpio-watch SECONDS
 */
typedef unsigned long u64; typedef unsigned int u32; typedef long s64;
#define NR_openat 56
#define NR_write 64
#define NR_exit 93
#define NR_mmap 222
#define NR_clock_gettime 113
static s64 sc6(s64 n, s64 a, s64 b, s64 c, s64 d, s64 e, s64 f) {
	register s64 x8 __asm__("x8") = n; register s64 x0 __asm__("x0") = a;
	register s64 x1 __asm__("x1") = b; register s64 x2 __asm__("x2") = c;
	register s64 x3 __asm__("x3") = d; register s64 x4 __asm__("x4") = e;
	register s64 x5 __asm__("x5") = f;
	__asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5) : "memory");
	return x0;
}
static void out(const char *s) { int n = 0; while (s[n]) n++; sc6(NR_write, 1, (s64)s, n, 0, 0, 0); }
static void outn(u64 v) { char b[24]; int i = 23; b[i] = 0; if (!v) b[--i] = '0'; while (v) { b[--i] = '0' + v % 10; v /= 10; } out(b + i); }
static u64 now_s(void) { s64 ts[2]; sc6(NR_clock_gettime, 1 /*MONOTONIC*/, (s64)ts, 0, 0, 0, 0); return ts[0]; }

static const u64 bank_base[5] = { 0xff610000, 0xffaf0000, 0xffb00000, 0xffb10000, 0xffb20000 };
static const char *bank_letter = "ABCD";
static u32 cnt[5][32];

/* entry: hand the initial stack pointer (argc, argv...) to C before any prologue moves sp */
__asm__(".global _start\n_start:\n\tmov x0, sp\n\tb cmain\n");

void cmain(s64 *sp) {
	s64 argc = sp[0]; char **argv = (char **)(sp + 1);
	u64 secs = 20;
	if (argc > 1) { secs = 0; for (char *p = argv[1]; *p >= '0' && *p <= '9'; p++) secs = secs * 10 + (*p - '0'); }
	s64 fd = sc6(NR_openat, -100, (s64)"/dev/mem", 2 | 04010000 /*O_RDWR|O_SYNC on aarch64 (010000000 would be O_PATH)*/, 0, 0, 0);
	if (fd < 0) { out("cannot open /dev/mem\n"); sc6(NR_exit, 1, 0, 0, 0, 0, 0); }
	volatile u32 *reg[5];
	for (int b = 0; b < 5; b++) {
		s64 m = sc6(NR_mmap, 0, 4096, 3 /*PROT_READ|PROT_WRITE, as busybox devmem*/, 1 /*MAP_SHARED*/, fd, bank_base[b]);
		if (m < 0 && m > -4096) {
			out("  bank "); outn(b); out(": mmap failed, errno "); outn(-m); out(" -- skipped\n");
			reg[b] = 0; continue;
		}
		reg[b] = (volatile u32 *)(m + 0x70);
	}
	u32 prev[5];
	for (int b = 0; b < 5; b++) prev[b] = reg[b] ? *reg[b] : 0;
	u64 end = now_s() + secs, samples = 0;
	out("gpio-watch: sampling for "); outn(secs); out(" s -- press the remote now\n");
	while (1) {
		for (int i = 0; i < 4096; i++) {
			for (int b = 0; b < 5; b++) {
				if (!reg[b]) continue;
				u32 v = *reg[b], x = v ^ prev[b];
				if (x) { for (int k = 0; k < 32; k++) if (x & (1u << k)) cnt[b][k]++; prev[b] = v; }
			}
		}
		samples += 4096;
		if (now_s() >= end) break;
	}
	out("samples per bank: "); outn(samples); out("\n");
	int any = 0;
	for (int b = 0; b < 5; b++) for (int k = 0; k < 32; k++) if (cnt[b][k]) {
		any = 1;
		out("  GPIO"); outn(b); out("_"); char l[2] = { bank_letter[k / 8], 0 }; out(l); outn(k % 8);
		out("  (bank "); outn(b); out(" pin "); outn(k); out(", linux gpio "); outn(b * 32 + k);
		out(")  transitions "); outn(cnt[b][k]); out("  now "); outn((prev[b] >> k) & 1); out("\n");
	}
	if (!any) out("  no pin changed\n");
	sc6(NR_exit, 0, 0, 0, 0, 0, 0);
	for (;;) ;
}
