// h96ui: the H96Max console interface on tty1 (HDMI). Lists IPTV subscriptions, imports them
// from a USB stick (no typing), plays channels with mpv, and keeps mpv settings.
// Driven by arrow keys / Enter / Back, which is what the IR remote will send on the console.
package main

import (
	"flag"
	"fmt"
	"os"
	"strings"
	"syscall"
	"time"
	"unicode/utf8"
)

var (
	version   = "0.1.1"
	buildDate = "dev"
)

type Item struct {
	Label  string
	Right  string
	Action func()
}

type Screen struct {
	Title   string
	Items   []Item
	Sel     int
	Top     int
	Empty   string // shown when there are no items
	Lines   []string
	OnBack  func()
	Refresh func(*Screen) // rebuild items when the screen is shown again
}

type App struct {
	t        *Term
	rows     int
	cols     int
	stack    []*Screen
	msg      string
	msgUntil time.Time
	wifi     WifiState
	wifiAt   time.Time
	settings Settings
	quitOK   bool
	snapshot bool
}

func (a *App) top() *Screen { return a.stack[len(a.stack)-1] }

func (a *App) push(s *Screen) {
	if s.Refresh != nil {
		s.Refresh(s)
	}
	a.stack = append(a.stack, s)
}

func (a *App) pop() {
	if len(a.stack) <= 1 {
		return
	}
	s := a.top()
	if s.OnBack != nil {
		s.OnBack()
	}
	a.stack = a.stack[:len(a.stack)-1]
	if p := a.top(); p.Refresh != nil {
		p.Refresh(p)
		if p.Sel >= len(p.Items) {
			p.Sel = max(0, len(p.Items)-1)
		}
	}
}

func (a *App) say(format string, args ...any) {
	a.msg = fmt.Sprintf(format, args...)
	a.msgUntil = time.Now().Add(6 * time.Second)
}

// ---------- rendering ----------

func fit(s string, w int) string {
	if w <= 0 {
		return ""
	}
	if utf8.RuneCountInString(s) <= w {
		return s + strings.Repeat(" ", w-utf8.RuneCountInString(s))
	}
	r := []rune(s)
	if w <= 1 {
		return string(r[:w])
	}
	return string(r[:w-1]) + "~"
}

const (
	cReset  = "\033[0m"
	cBar    = "\033[0;30;46m" // black on cyan
	cTitle  = "\033[1;37m"
	cSel    = "\033[0;30;47m" // black on white
	cDim    = "\033[0;37m"
	cMsg    = "\033[1;33m"
	cFooter = "\033[0;30;46m"
)

func (a *App) header() string {
	if time.Since(a.wifiAt) > 5*time.Second || a.wifiAt.IsZero() {
		a.wifi = readWifi()
		a.wifiAt = time.Now()
	}
	left := "  WiFi: not connected"
	if a.wifi.SSID != "" {
		left = fmt.Sprintf("  WiFi: %s  %s  %s", a.wifi.SSID, a.wifi.Signal, a.wifi.IP)
	}
	right := time.Now().Format("Mon 02 Jan  15:04") + "  "
	gap := a.cols - utf8.RuneCountInString(left) - utf8.RuneCountInString(right)
	if gap < 1 {
		left = fit(left, a.cols-utf8.RuneCountInString(right)-1)
		gap = 1
	}
	return cBar + left + strings.Repeat(" ", gap) + right + cReset
}

func (a *App) footer() string {
	left := fmt.Sprintf("  h96ui %s (%s)  |  Alpine %s  |  Linux %s", version, buildDate, alpineRelease(), kernelRelease())
	right := "Up/Down  OK  Back  "
	gap := a.cols - utf8.RuneCountInString(left) - utf8.RuneCountInString(right)
	if gap < 1 {
		return cFooter + fit(left, a.cols) + cReset
	}
	return cFooter + left + strings.Repeat(" ", gap) + right + cReset
}

func (a *App) render() string {
	s := a.top()
	var b strings.Builder
	b.WriteString("\033[H")
	line := func(txt string) { b.WriteString(txt); b.WriteString("\033[K\r\n") }

	line(a.header())
	line("")
	crumbs := make([]string, 0, len(a.stack))
	for _, sc := range a.stack {
		crumbs = append(crumbs, sc.Title)
	}
	line(cTitle + "  " + fit(strings.Join(crumbs, "  >  "), a.cols-4) + cReset)
	line(cDim + "  " + strings.Repeat("-", a.cols-4) + cReset)

	body := a.rows - 4 - 3 // header+blank+title+rule, message+blank+footer
	used := 0
	if len(s.Lines) > 0 {
		for _, l := range s.Lines {
			if used >= body {
				break
			}
			line("  " + fit(l, a.cols-4))
			used++
		}
	}
	if len(s.Items) == 0 && len(s.Lines) == 0 && used < body {
		line(cDim + "  " + fit(s.Empty, a.cols-4) + cReset)
		used++
	}
	listRows := body - used
	if listRows > 0 && len(s.Items) > 0 {
		if s.Sel < s.Top {
			s.Top = s.Sel
		}
		if s.Sel >= s.Top+listRows {
			s.Top = s.Sel - listRows + 1
		}
		for i := s.Top; i < len(s.Items) && i < s.Top+listRows; i++ {
			it := s.Items[i]
			w := a.cols - 8
			lw := w - utf8.RuneCountInString(it.Right) - 2
			txt := fit(it.Label, lw) + "  " + it.Right
			if i == s.Sel {
				line("  " + cSel + "> " + fit(txt, w) + " " + cReset)
			} else {
				line("    " + fit(txt, w))
			}
			used++
		}
	}
	for ; used < body; used++ {
		line("")
	}
	msg := ""
	if time.Now().Before(a.msgUntil) {
		msg = a.msg
	} else if len(s.Items) > 0 {
		msg = fmt.Sprintf("%d / %d", s.Sel+1, len(s.Items))
	}
	line(cMsg + "  " + fit(msg, a.cols-4) + cReset)
	line("")
	b.WriteString(a.footer())
	b.WriteString("\033[K")
	return b.String()
}

func (a *App) draw() {
	if a.snapshot {
		return
	}
	// the console can change size under us (h96-console sets 1080p + a 16x32 font at boot)
	if r, c := a.t.Size(); r != a.rows || c != a.cols {
		a.rows, a.cols = r, c
		os.Stdout.WriteString("\033[2J")
	}
	os.Stdout.WriteString(a.render())
}

// ---------- input ----------

func (a *App) readKey(timeout time.Duration) Key {
	var rfds syscall.FdSet
	fd := int(os.Stdin.Fd())
	rfds.Bits[fd/64] |= 1 << (uint(fd) % 64)
	tv := syscall.NsecToTimeval(int64(timeout))
	n, err := syscall.Select(fd+1, &rfds, nil, nil, &tv)
	if err != nil || n <= 0 {
		return KeyNone
	}
	buf := make([]byte, 16)
	m, err := os.Stdin.Read(buf)
	if err != nil || m <= 0 {
		return KeyNone
	}
	return decodeKey(buf[:m])
}

func (a *App) handle(k Key) bool {
	s := a.top()
	n := len(s.Items)
	page := max(1, a.rows-10)
	switch k {
	case KeyUp:
		if n > 0 {
			s.Sel = (s.Sel - 1 + n) % n
		}
	case KeyDown:
		if n > 0 {
			s.Sel = (s.Sel + 1) % n
		}
	case KeyPgUp:
		s.Sel = max(0, s.Sel-page)
	case KeyPgDn:
		s.Sel = min(max(0, n-1), s.Sel+page)
	case KeyHome:
		s.Sel = 0
	case KeyEnd:
		s.Sel = max(0, n-1)
	case KeyEnter, KeyRight:
		if n > 0 && s.Items[s.Sel].Action != nil {
			s.Items[s.Sel].Action()
		}
	case KeyBack, KeyLeft:
		a.pop()
	case KeyQuit:
		return !a.quitOK
	}
	return true
}

// runExternal hands the terminal to another program (mpv) and takes it back.
func (a *App) runExternal(f func() error) error {
	a.t.Restore()
	os.Stdout.WriteString("\033[0m\033[2J\033[H\033[?25h")
	err := f()
	a.t.Raw()
	os.Stdout.WriteString("\033[?25l\033[2J")
	return err
}

func main() {
	snap := flag.String("snapshot", "", "render a screen as text and exit: main|subs|channels|settings|about|usb")
	quit := flag.Bool("allow-quit", false, "let 'q' leave the program (development)")
	showVer := flag.Bool("version", false, "print the version")
	flag.Parse()
	if *showVer {
		fmt.Printf("h96ui %s (%s)\n", version, buildDate)
		return
	}
	a := &App{settings: LoadSettings(), quitOK: *quit, rows: 36, cols: 90}
	a.push(a.mainMenu())

	if *snap != "" {
		a.snapshot = true
		a.goSnapshot(*snap)
		out := a.render()
		fmt.Print(out + "\n")
		return
	}

	t, err := NewTerm(os.Stdin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "h96ui: not a terminal:", err)
		os.Exit(1)
	}
	a.t = t
	a.rows, a.cols = t.Size()
	if err := t.Raw(); err != nil {
		fmt.Fprintln(os.Stderr, "h96ui: raw mode:", err)
		os.Exit(1)
	}
	defer func() {
		t.Restore()
		os.Stdout.WriteString("\033[0m\033[2J\033[H\033[?25h")
	}()
	// console: never blank, hide the cursor
	os.Stdout.WriteString("\033[9;0]\033[14;0]\033[?25l\033[2J")
	lastMin := -1
	a.draw()
	for {
		k := a.readKey(time.Second)
		if k != KeyNone {
			if !a.handle(k) {
				return
			}
			a.draw()
			continue
		}
		// idle tick: redraw for the clock / WiFi / message expiry
		if m := time.Now().Minute(); m != lastMin || time.Now().Before(a.msgUntil.Add(time.Second)) || time.Since(a.wifiAt) > 5*time.Second {
			lastMin = m
			a.draw()
		}
	}
}
