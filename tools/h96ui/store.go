package main

// Subscriptions and settings under /var/lib/h96ui (override with H96UI_DATA for tests).
//   subs/<name>.m3u   the playlist (copied from USB, or downloaded)
//   subs/<name>.url   only for URL subscriptions: the source URL (used by Refresh)
//   settings.json     mpv playback settings

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
)

func dataDir() string {
	if d := os.Getenv("H96UI_DATA"); d != "" {
		return d
	}
	return "/var/lib/h96ui"
}

func subsDir() string { return filepath.Join(dataDir(), "subs") }

type Subscription struct {
	Name string
	Path string // .m3u
	URL  string // empty for file subscriptions
}

func ListSubscriptions() []Subscription {
	var out []Subscription
	files, _ := filepath.Glob(filepath.Join(subsDir(), "*.m3u"))
	for _, f := range files {
		name := strings.TrimSuffix(filepath.Base(f), ".m3u")
		s := Subscription{Name: name, Path: f}
		if b, err := os.ReadFile(strings.TrimSuffix(f, ".m3u") + ".url"); err == nil {
			s.URL = strings.TrimSpace(string(b))
		}
		out = append(out, s)
	}
	sort.Slice(out, func(i, j int) bool { return strings.ToLower(out[i].Name) < strings.ToLower(out[j].Name) })
	return out
}

func (s Subscription) Channels() ([]Channel, error) {
	f, err := os.Open(s.Path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	return ParseM3U(f), nil
}

var unsafeName = regexp.MustCompile(`[^A-Za-z0-9._ -]+`)

func uniqueName(base string) string {
	base = strings.TrimSpace(unsafeName.ReplaceAllString(base, "_"))
	if base == "" {
		base = "subscription"
	}
	name := base
	for i := 2; ; i++ {
		if _, err := os.Stat(filepath.Join(subsDir(), name+".m3u")); os.IsNotExist(err) {
			return name
		}
		name = fmt.Sprintf("%s (%d)", base, i)
	}
}

// ImportFile copies a playlist file into the store; returns the subscription name and channel count.
func ImportFile(src string) (string, int, error) {
	b, err := os.ReadFile(src)
	if err != nil {
		return "", 0, err
	}
	n := len(ParseM3U(strings.NewReader(string(b))))
	if n == 0 {
		return "", 0, fmt.Errorf("no channels found in %s", filepath.Base(src))
	}
	if err := os.MkdirAll(subsDir(), 0755); err != nil {
		return "", 0, err
	}
	ext := filepath.Ext(src)
	name := uniqueName(strings.TrimSuffix(filepath.Base(src), ext))
	if err := os.WriteFile(filepath.Join(subsDir(), name+".m3u"), b, 0644); err != nil {
		return "", 0, err
	}
	return name, n, nil
}

// URLsIn returns the http(s) URLs in a .txt/.url file (one per line, # comments allowed).
func URLsIn(src string) []string {
	b, err := os.ReadFile(src)
	if err != nil {
		return nil
	}
	var out []string
	for _, l := range strings.Split(string(b), "\n") {
		l = strings.TrimSpace(strings.TrimPrefix(l, "\ufeff"))
		if strings.HasPrefix(l, "http://") || strings.HasPrefix(l, "https://") {
			out = append(out, l)
		}
	}
	return out
}

func fetch(url string) ([]byte, error) {
	c := &http.Client{Timeout: 60 * time.Second}
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "h96ui/"+version)
	resp, err := c.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("HTTP %d", resp.StatusCode)
	}
	return io.ReadAll(io.LimitReader(resp.Body, 64<<20))
}

// ImportURL downloads a playlist and stores it as a refreshable URL subscription.
func ImportURL(url string) (string, int, error) {
	b, err := fetch(url)
	if err != nil {
		return "", 0, err
	}
	n := len(ParseM3U(strings.NewReader(string(b))))
	if n == 0 {
		return "", 0, fmt.Errorf("no channels in the downloaded playlist")
	}
	if err := os.MkdirAll(subsDir(), 0755); err != nil {
		return "", 0, err
	}
	base := filepath.Base(strings.Split(url, "?")[0])
	base = strings.TrimSuffix(strings.TrimSuffix(base, ".m3u8"), ".m3u")
	name := uniqueName(base)
	if err := os.WriteFile(filepath.Join(subsDir(), name+".m3u"), b, 0644); err != nil {
		return "", 0, err
	}
	if err := os.WriteFile(filepath.Join(subsDir(), name+".url"), []byte(url+"\n"), 0644); err != nil {
		return "", 0, err
	}
	return name, n, nil
}

func (s Subscription) Refresh() (int, error) {
	if s.URL == "" {
		return 0, fmt.Errorf("not a URL subscription")
	}
	b, err := fetch(s.URL)
	if err != nil {
		return 0, err
	}
	n := len(ParseM3U(strings.NewReader(string(b))))
	if n == 0 {
		return 0, fmt.Errorf("no channels in the downloaded playlist")
	}
	return n, os.WriteFile(s.Path, b, 0644)
}

func (s Subscription) Remove() error {
	os.Remove(strings.TrimSuffix(s.Path, ".m3u") + ".url")
	return os.Remove(s.Path)
}

// ---- settings ----

type Settings struct {
	CacheSecs   int    `json:"cache_secs"`
	Deinterlace bool   `json:"deinterlace"`
	Volume      int    `json:"volume"`
	Audio       string `json:"audio"`  // hdmi | spdif | analog
	Aspect      string `json:"aspect"` // fit | stretch | fill
}

func DefaultSettings() Settings {
	return Settings{CacheSecs: 5, Deinterlace: false, Volume: 80, Audio: "hdmi", Aspect: "fit"}
}

func LoadSettings() Settings {
	s := DefaultSettings()
	if b, err := os.ReadFile(filepath.Join(dataDir(), "settings.json")); err == nil {
		json.Unmarshal(b, &s)
	}
	return s
}

func (s Settings) Save() error {
	if err := os.MkdirAll(dataDir(), 0755); err != nil {
		return err
	}
	b, _ := json.MarshalIndent(s, "", "  ")
	return os.WriteFile(filepath.Join(dataDir(), "settings.json"), append(b, '\n'), 0644)
}
