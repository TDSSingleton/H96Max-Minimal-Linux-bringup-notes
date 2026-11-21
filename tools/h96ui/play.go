package main

// Playback: hand the screen to mpv (DRM output, no GPU/X/Wayland needed), then take it back.
// Channel up/down = the subscription's playlist; Esc/Backspace/q returns to the menu.

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
)

const mpvInputConf = `# h96ui: keys while playing (the IR remote will map onto these)
ESC quit
BS quit
q quit
UP playlist-prev
DOWN playlist-next
PGUP playlist-prev
PGDWN playlist-next
LEFT seek -10
RIGHT seek 10
ENTER show-text "${playlist-pos-1}/${playlist-count}  ${media-title}" 3000
SPACE cycle pause
`

func audioDevice(a string) string {
	switch a {
	case "spdif":
		return "alsa/plughw:CARD=ROCKCHIPSPDIF,DEV=0"
	case "analog":
		return "alsa/plughw:CARD=rk3528acodec,DEV=0"
	}
	return "alsa/plughw:CARD=rockchiphdmi,DEV=0"
}

func mpvArgs(s Settings, playlist string, index int, inputConf string) []string {
	a := []string{
		"--vo=drm", "--hwdec=no", "--fullscreen",
		"--input-terminal=yes", "--input-default-bindings=no", "--input-conf=" + inputConf,
		"--term-osd=no", "--msg-level=all=error", "--really-quiet",
		"--osd-level=1", "--osd-duration=2500",
		"--cache=yes", "--cache-secs=" + strconv.Itoa(s.CacheSecs),
		"--demuxer-readahead-secs=" + strconv.Itoa(s.CacheSecs),
		"--network-timeout=20",
		"--volume=" + strconv.Itoa(s.Volume),
		"--audio-device=" + audioDevice(s.Audio),
		"--playlist=" + playlist, "--playlist-start=" + strconv.Itoa(index),
	}
	if s.Deinterlace {
		a = append(a, "--deinterlace=yes")
	}
	switch s.Aspect {
	case "stretch":
		a = append(a, "--keepaspect=no")
	case "fill":
		a = append(a, "--panscan=1.0")
	}
	return a
}

// Play runs mpv in the foreground on our terminal. The caller restores/re-enters raw mode.
func Play(s Settings, sub Subscription, index int) error {
	conf := filepath.Join(dataDir(), "mpv-input.conf")
	os.MkdirAll(dataDir(), 0755)
	if err := os.WriteFile(conf, []byte(mpvInputConf), 0644); err != nil {
		return err
	}
	if _, err := exec.LookPath("mpv"); err != nil {
		return fmt.Errorf("mpv is not installed (apk add mpv)")
	}
	cmd := exec.Command("mpv", mpvArgs(s, sub.Path, index, conf)...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	return cmd.Run()
}
