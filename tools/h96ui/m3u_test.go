package main

import (
	"strings"
	"testing"
)

func TestParseM3U(t *testing.T) {
	in := "\ufeff#EXTM3U\n" +
		"#EXTINF:-1 tvg-id=\"a\" group-title=\"News, World\",Channel One, HD\n" +
		"http://example.invalid/one.m3u8\n\n" +
		"#EXTINF:-1,Plain Two\n#EXTGRP:Music\nhttps://example.invalid/two\n" +
		"http://example.invalid/three\n" +
		"#EXTINF:-1 tvg-name=\"Four\" group-title=\"Kids\",\nhttp://example.invalid/four\n"
	ch := ParseM3U(strings.NewReader(in))
	if len(ch) != 4 {
		t.Fatalf("want 4 channels, got %d: %+v", len(ch), ch)
	}
	exp := []Channel{
		{"Channel One, HD", "News, World", "http://example.invalid/one.m3u8"},
		{"Plain Two", "Music", "https://example.invalid/two"},
		{"http://example.invalid/three", "", "http://example.invalid/three"},
		{"Four", "Kids", "http://example.invalid/four"},
	}
	for i := range exp {
		if ch[i] != exp[i] {
			t.Errorf("channel %d: got %+v want %+v", i, ch[i], exp[i])
		}
	}
}

func TestDecodeKey(t *testing.T) {
	cases := map[string]Key{"\x1b[A": KeyUp, "\x1b[B": KeyDown, "\r": KeyEnter, "\x1b": KeyBack,
		"\x7f": KeyBack, "\x1b[5~": KeyPgUp, "\x1b[6~": KeyPgDn, "x": KeyNone}
	for in, want := range cases {
		if got := decodeKey([]byte(in)); got != want {
			t.Errorf("%q: got %v want %v", in, got, want)
		}
	}
}
