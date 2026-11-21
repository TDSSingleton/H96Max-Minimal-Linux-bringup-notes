package main

// Minimal terminal handling for the Linux console (tty1): raw mode via termios ioctls,
// size via TIOCGWINSZ, and key decoding. Standard library only.

import (
	"os"
	"syscall"
	"unsafe"
)

type Key int

const (
	KeyNone Key = iota
	KeyUp
	KeyDown
	KeyLeft
	KeyRight
	KeyEnter
	KeyBack // Esc, Backspace
	KeyPgUp
	KeyPgDn
	KeyHome
	KeyEnd
	KeyQuit // 'q' (keyboard only; the remote has no q)
)

type Term struct {
	fd    int
	saved syscall.Termios
	raw   bool
}

func ioctl(fd int, req uintptr, arg unsafe.Pointer) error {
	_, _, e := syscall.Syscall(syscall.SYS_IOCTL, uintptr(fd), req, uintptr(arg))
	if e != 0 {
		return e
	}
	return nil
}

func NewTerm(f *os.File) (*Term, error) {
	t := &Term{fd: int(f.Fd())}
	if err := ioctl(t.fd, syscall.TCGETS, unsafe.Pointer(&t.saved)); err != nil {
		return nil, err
	}
	return t, nil
}

func (t *Term) Raw() error {
	r := t.saved
	r.Iflag &^= syscall.IGNBRK | syscall.BRKINT | syscall.PARMRK | syscall.ISTRIP |
		syscall.INLCR | syscall.IGNCR | syscall.ICRNL | syscall.IXON
	r.Lflag &^= syscall.ECHO | syscall.ECHONL | syscall.ICANON | syscall.ISIG | syscall.IEXTEN
	r.Cflag &^= syscall.CSIZE | syscall.PARENB
	r.Cflag |= syscall.CS8
	r.Cc[syscall.VMIN] = 1
	r.Cc[syscall.VTIME] = 0
	if err := ioctl(t.fd, syscall.TCSETS, unsafe.Pointer(&r)); err != nil {
		return err
	}
	t.raw = true
	return nil
}

func (t *Term) Restore() {
	if t.raw {
		ioctl(t.fd, syscall.TCSETS, unsafe.Pointer(&t.saved))
		t.raw = false
	}
}

// Size returns rows, cols (falls back to the box's 90x36 console).
func (t *Term) Size() (int, int) {
	var ws struct{ Row, Col, X, Y uint16 }
	if err := ioctl(t.fd, syscall.TIOCGWINSZ, unsafe.Pointer(&ws)); err != nil || ws.Row == 0 {
		return 36, 90
	}
	return int(ws.Row), int(ws.Col)
}

// decodeKey turns one read() worth of bytes into a key. The Linux console sends each escape
// sequence in a single write, so a lone ESC in a read is the Esc key itself.
func decodeKey(b []byte) Key {
	s := string(b)
	switch s {
	case "\x1b[A", "\x1bOA":
		return KeyUp
	case "\x1b[B", "\x1bOB":
		return KeyDown
	case "\x1b[C", "\x1bOC":
		return KeyRight
	case "\x1b[D", "\x1bOD":
		return KeyLeft
	case "\r", "\n", " ":
		return KeyEnter
	case "\x1b", "\x7f", "\x08":
		return KeyBack
	case "\x1b[5~":
		return KeyPgUp
	case "\x1b[6~":
		return KeyPgDn
	case "\x1b[1~", "\x1b[H", "\x1bOH":
		return KeyHome
	case "\x1b[4~", "\x1b[F", "\x1bOF":
		return KeyEnd
	case "q", "Q":
		return KeyQuit
	}
	return KeyNone
}
