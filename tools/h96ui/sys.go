package main

// Things the UI reads from the system: WiFi state, the USB stick, version strings.

import (
	"bufio"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type WifiState struct {
	SSID   string
	Signal string // e.g. "-58 dBm"
	IP     string
}

func readWifi() WifiState {
	var w WifiState
	out, err := exec.Command("iw", "dev", "wlan0", "link").Output()
	if err == nil {
		sc := bufio.NewScanner(strings.NewReader(string(out)))
		for sc.Scan() {
			l := strings.TrimSpace(sc.Text())
			switch {
			case strings.HasPrefix(l, "SSID:"):
				w.SSID = strings.TrimSpace(strings.TrimPrefix(l, "SSID:"))
			case strings.HasPrefix(l, "signal:"):
				w.Signal = strings.TrimSpace(strings.TrimPrefix(l, "signal:"))
			}
		}
	}
	if out, err := exec.Command("ip", "-4", "-o", "addr", "show", "wlan0").Output(); err == nil {
		f := strings.Fields(string(out))
		for i := range f {
			if f[i] == "inet" && i+1 < len(f) {
				w.IP = strings.Split(f[i+1], "/")[0]
			}
		}
	}
	return w
}

func readFirstLine(p string) string {
	b, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(strings.SplitN(string(b), "\n", 2)[0])
}

func kernelRelease() string { return readFirstLine("/proc/sys/kernel/osrelease") }
func alpineRelease() string { return readFirstLine("/etc/alpine-release") }

// ---- USB stick ----

const usbMount = "/mnt/h96usb"

// MountUSB mounts the first USB stick partition read-only. Returns "" if none.
func MountUSB() (string, error) {
	UnmountUSB()
	os.MkdirAll(usbMount, 0755)
	cands, _ := filepath.Glob("/dev/sd[a-z][0-9]")
	whole, _ := filepath.Glob("/dev/sd[a-z]")
	cands = append(cands, whole...)
	var lastErr error
	for _, dev := range cands {
		for _, fs := range []string{"vfat", "exfat", "ext4"} {
			if err := exec.Command("mount", "-t", fs, "-o", "ro", dev, usbMount).Run(); err == nil {
				return dev, nil
			} else {
				lastErr = err
			}
		}
	}
	return "", lastErr
}

func UnmountUSB() { exec.Command("umount", usbMount).Run() }

type Importable struct {
	Path string
	Kind string // "playlist" | "urls"
	Note string
}

// FindImportables lists playlist files and URL lists on the stick (root and /iptv).
func FindImportables() []Importable {
	var out []Importable
	for _, dir := range []string{usbMount, filepath.Join(usbMount, "iptv"), filepath.Join(usbMount, "IPTV")} {
		ents, err := os.ReadDir(dir)
		if err != nil {
			continue
		}
		for _, e := range ents {
			if e.IsDir() || strings.HasPrefix(e.Name(), ".") {
				continue
			}
			p := filepath.Join(dir, e.Name())
			switch strings.ToLower(filepath.Ext(e.Name())) {
			case ".m3u", ".m3u8":
				out = append(out, Importable{p, "playlist", ""})
			case ".txt", ".url":
				if u := URLsIn(p); len(u) > 0 {
					out = append(out, Importable{p, "urls", ""})
				}
			}
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Path < out[j].Path })
	return out
}
