package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

func (a *App) mainMenu() *Screen {
	s := &Screen{Title: "H96Max"}
	s.Refresh = func(s *Screen) {
		n := len(ListSubscriptions())
		s.Items = []Item{
			{Label: "Subscriptions", Right: fmt.Sprintf("%d", n), Action: func() { a.push(a.subsScreen()) }},
			{Label: "Add subscription from USB", Action: func() { a.push(a.usbScreen()) }},
			{Label: "Settings", Action: func() { a.push(a.settingsScreen()) }},
		}
	}
	return s
}

func (a *App) subsScreen() *Screen {
	s := &Screen{Title: "Subscriptions", Empty: "No subscriptions yet. Use 'Add subscription from USB'."}
	s.Refresh = func(s *Screen) {
		s.Items = nil
		for _, sub := range ListSubscriptions() {
			sub := sub
			right := "file"
			if sub.URL != "" {
				right = "url"
			}
			if ch, err := sub.Channels(); err == nil {
				right = fmt.Sprintf("%d channels  %s", len(ch), right)
			}
			s.Items = append(s.Items, Item{Label: sub.Name, Right: right, Action: func() { a.push(a.subActions(sub)) }})
		}
	}
	return s
}

func (a *App) subActions(sub Subscription) *Screen {
	s := &Screen{Title: sub.Name}
	s.Items = []Item{{Label: "Channels", Action: func() { a.push(a.channelsScreen(sub)) }}}
	if sub.URL != "" {
		s.Items = append(s.Items, Item{Label: "Refresh from URL", Right: shortURL(sub.URL), Action: func() {
			a.say("Downloading %s ...", shortURL(sub.URL))
			a.draw()
			if n, err := sub.Refresh(); err != nil {
				a.say("Refresh failed: %v", err)
			} else {
				a.say("Refreshed: %d channels", n)
			}
		}})
	}
	s.Items = append(s.Items, Item{Label: "Remove", Action: func() {
		a.push(&Screen{Title: "Remove " + sub.Name + "?", Items: []Item{
			{Label: "No, keep it", Action: func() { a.pop() }},
			{Label: "Yes, remove it", Action: func() {
				if err := sub.Remove(); err != nil {
					a.say("Remove failed: %v", err)
				} else {
					a.say("Removed %s", sub.Name)
				}
				a.pop()
				a.pop()
			}},
		}})
	}})
	return s
}

func shortURL(u string) string {
	u = strings.TrimPrefix(strings.TrimPrefix(u, "https://"), "http://")
	if len(u) > 40 {
		u = u[:37] + "..."
	}
	return u
}

func (a *App) channelsScreen(sub Subscription) *Screen {
	s := &Screen{Title: "Channels", Empty: "This playlist has no channels."}
	ch, err := sub.Channels()
	if err != nil {
		s.Empty = "Cannot read the playlist: " + err.Error()
	}
	for i, c := range ch {
		i := i
		s.Items = append(s.Items, Item{Label: c.Name, Right: c.Group, Action: func() {
			err := a.runExternal(func() error { return Play(a.settings, sub, i) })
			if err != nil {
				a.say("Playback: %v", err)
			}
		}})
	}
	return s
}

func (a *App) usbScreen() *Screen {
	s := &Screen{Title: "Add from USB"}
	s.OnBack = UnmountUSB
	s.Refresh = func(s *Screen) {
		s.Items, s.Lines = nil, nil
		dev, err := MountUSB()
		if dev == "" {
			s.Empty = "No USB stick found. Plug one in (FAT/exFAT/ext4) and press OK to retry."
			if err != nil {
				s.Empty += "  (" + err.Error() + ")"
			}
			s.Items = []Item{{Label: "Retry", Action: func() { s.Refresh(s) }}}
			return
		}
		files := FindImportables()
		s.Lines = []string{fmt.Sprintf("USB stick %s: .m3u/.m3u8 playlists, and .txt/.url files with playlist URLs", dev), ""}
		if len(files) == 0 {
			s.Lines = append(s.Lines, "Nothing importable in / or /iptv on the stick.")
		}
		for _, f := range files {
			f := f
			rel := strings.TrimPrefix(f.Path, usbMount+"/")
			right := "playlist"
			if f.Kind == "urls" {
				right = fmt.Sprintf("%d URL(s)", len(URLsIn(f.Path)))
			}
			s.Items = append(s.Items, Item{Label: rel, Right: right, Action: func() { a.importOne(f) }})
		}
	}
	return s
}

func (a *App) importOne(f Importable) {
	if f.Kind == "playlist" {
		name, n, err := ImportFile(f.Path)
		if err != nil {
			a.say("Import failed: %v", err)
			return
		}
		a.say("Imported '%s': %d channels", name, n)
		return
	}
	ok, bad := 0, 0
	var last error
	for _, u := range URLsIn(f.Path) {
		a.say("Downloading %s ...", shortURL(u))
		a.draw()
		if _, _, err := ImportURL(u); err != nil {
			bad++
			last = err
		} else {
			ok++
		}
	}
	if bad > 0 {
		a.say("Imported %d, failed %d (last error: %v)", ok, bad, last)
	} else {
		a.say("Imported %d URL subscription(s)", ok)
	}
}

func (a *App) settingsScreen() *Screen {
	s := &Screen{Title: "Settings"}
	cycleInt := func(cur int, opts []int) int {
		for i, o := range opts {
			if o == cur {
				return opts[(i+1)%len(opts)]
			}
		}
		return opts[0]
	}
	cycleStr := func(cur string, opts []string) string {
		for i, o := range opts {
			if o == cur {
				return opts[(i+1)%len(opts)]
			}
		}
		return opts[0]
	}
	save := func() {
		if err := a.settings.Save(); err != nil {
			a.say("Could not save settings: %v", err)
		}
	}
	onoff := map[bool]string{true: "on", false: "off"}
	s.Refresh = func(s *Screen) {
		st := &a.settings
		s.Items = []Item{
			{Label: "Network cache", Right: fmt.Sprintf("%d s", st.CacheSecs), Action: func() {
				st.CacheSecs = cycleInt(st.CacheSecs, []int{2, 5, 10, 20}); save(); s.Refresh(s)
			}},
			{Label: "Deinterlace", Right: onoff[st.Deinterlace], Action: func() {
				st.Deinterlace = !st.Deinterlace; save(); s.Refresh(s)
			}},
			{Label: "Start volume", Right: strconv.Itoa(st.Volume), Action: func() {
				st.Volume = cycleInt(st.Volume, []int{50, 60, 70, 80, 90, 100}); save(); s.Refresh(s)
			}},
			{Label: "Audio output", Right: strings.ToUpper(st.Audio), Action: func() {
				st.Audio = cycleStr(st.Audio, []string{"hdmi", "spdif", "analog"}); save(); s.Refresh(s)
			}},
			{Label: "Picture", Right: st.Aspect, Action: func() {
				st.Aspect = cycleStr(st.Aspect, []string{"fit", "stretch", "fill"}); save(); s.Refresh(s)
			}},
			{Label: "Video decoding", Right: "software (hardware: later)", Action: func() {
				a.say("Hardware decoding (Rockchip MPP) is not wired up yet.")
			}},
			{Label: "About this box", Action: func() { a.push(a.aboutScreen()) }},
		}
	}
	return s
}

func (a *App) aboutScreen() *Screen {
	s := &Screen{Title: "About"}
	s.Refresh = func(s *Screen) {
		up := readFirstLine("/proc/uptime")
		if f := strings.Fields(up); len(f) > 0 {
			if sec, err := strconv.ParseFloat(f[0], 64); err == nil {
				up = (time.Duration(sec) * time.Second).String()
			}
		}
		temp := readFirstLine("/sys/class/thermal/thermal_zone0/temp")
		if v, err := strconv.Atoi(temp); err == nil {
			temp = fmt.Sprintf("%d C", v/1000)
		}
		mem := ""
		if b, err := os.ReadFile("/proc/meminfo"); err == nil {
			var tot, av int
			for _, l := range strings.Split(string(b), "\n") {
				fmt.Sscanf(l, "MemTotal: %d kB", &tot)
				fmt.Sscanf(l, "MemAvailable: %d kB", &av)
			}
			mem = fmt.Sprintf("%d / %d MiB used", (tot-av)/1024, tot/1024)
		}
		emmc := ""
		if blks, _ := filepath.Glob("/sys/block/mmcblk[0-9]"); len(blks) > 0 {
			for _, b := range blks {
				if readFirstLine(b+"/device/type") == "MMC" {
					emmc = "life_time " + readFirstLine(b+"/device/life_time") + "  pre_eol " + readFirstLine(b+"/device/pre_eol_info")
				}
			}
		}
		w := readWifi()
		s.Lines = []string{
			"Model      " + strings.TrimRight(readFirstLine("/proc/device-tree/model"), "\x00"),
			"Software   h96ui " + version + " (" + buildDate + "), Alpine " + alpineRelease() + ", Linux " + kernelRelease(),
			"Uptime     " + up,
			"CPU temp   " + temp,
			"Memory     " + mem,
			"eMMC       " + emmc,
			"WiFi       " + w.SSID + "  " + w.Signal + "  " + w.IP,
			"Data       " + dataDir(),
		}
	}
	return s
}

// goSnapshot navigates to a named screen for --snapshot (layout checks without the TV).
func (a *App) goSnapshot(name string) {
	switch name {
	case "subs":
		a.push(a.subsScreen())
	case "channels":
		if subs := ListSubscriptions(); len(subs) > 0 {
			a.push(a.subsScreen())
			a.push(a.channelsScreen(subs[0]))
		}
	case "settings":
		a.push(a.settingsScreen())
	case "about":
		a.push(a.settingsScreen())
		a.push(a.aboutScreen())
	case "usb":
		a.push(a.usbScreen())
	}
}
